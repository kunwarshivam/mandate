//! A connection's stream rules (journal spec v0.32 §9.8 rules 66 to 68 and 131) and §11's two
//! connection checks, `connection_lifecycle_mismatch` and `connection_cause_mismatch` (E7-17, DEC-800 item 9).
//! A range's lifecycle run starts from its connection anchor, or fails closed without one (journal
//! spec v0.35, DEC-885).
//! `append` folds no stream, so these rules are not `append`'s: the connection manager and the
//! executor check them against their own fold before appending, and verification runs them here
//! over the stored rows.
//!
//! Each function takes the rows of the control stream and of the account streams together, in
//! commit order, from each stream's first event: rules 66, 67, and 131 fold the control stream,
//! rule 68 folds each account stream on its own, and the cause check follows a `causation_id` from
//! one stream to the other. Rows of other event types are skipped, and the first failing row is
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

use crate::{StoredEvent, StreamId, StreamType, VerifiedPrefix};

const ESTABLISHED: &str = "ConnectionEstablished";
const REVOKED: &str = "ConnectionRevoked";
const ROTATED: &str = "ConnectionCredentialRotated";
const REFUSED: &str = "ConnectionRefused";
const CHECKED: &str = "ConnectionChecked";
const REQUESTED: &str = "ConnectionRequested";

/// The connection records on the control stream, which the connection manager folds.
const CONTROL_RECORDS: [&str; 5] = [ESTABLISHED, REVOKED, ROTATED, REFUSED, REQUESTED];

/// What a connect's establishment repeats of the request it closes (rule 131).
const ESTABLISHMENT_REPEATS: [&str; 5] =
    ["account_ref", "broker", "environment", "user", "step_up"];

/// What a connect's refusal repeats of the request it closes (rule 131).
const REFUSAL_REPEATS: [&str; 4] = ["broker", "environment", "user", "step_up"];

/// The connection records on an account stream, which its executor folds: a `ConnectionRevoked`
/// among them only so rule 68 refuses it there (journal spec v0.37, DEC-888).
const ACCOUNT_RECORDS: [&str; 6] = [
    CHECKED,
    "ConnectionStateChanged",
    "ConnectionCredentialRefreshed",
    ESTABLISHED,
    ROTATED,
    REVOKED,
];

/// The control-stream records the range rules judge (rules 66, 67, and 131): the set an unanchored
/// range fails closed at there (journal spec v0.35 §11, DEC-885 item 4). A `ConnectionRevoked` on a
/// control stream is never judged (DEC-885 I6).
pub const JUDGED_ON_CONTROL: &[&str] = &[REQUESTED, ESTABLISHED, ROTATED, REFUSED];

/// The connection records on an account stream, every one of which rule 68 judges and an
/// unanchored range fails closed at (journal spec v0.37 §11, DEC-888): the records the executor's
/// fold reads, since a type it skips is judged by nothing, a `ConnectionRevoked` included.
pub const JUDGED_ON_ACCOUNT: &[&str] = &ACCOUNT_RECORDS;

/// The occasions an account stream's checks may have before its first binding (rule 68).
const UNBOUND_OCCASIONS: [&str; 3] = ["connect", "reconnect", "reauthorize"];

/// The brokers that connect through MCP (journal spec §9.8), whose checks always list `contract`.
const MCP_BROKERS: [&str; 1] = ["robinhood"];

/// A §9.8 stream rule: 66 (establishment), 67 (rotation and refusal), 68 (the account stream), and
/// 131 (the pending connection, journal spec v0.32, DEC-699).
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
    /// From a control stream's first `ConnectionRequested` on, every connect closes its own open
    /// request once, repeating its members, and a request names a new connection and `account_ref`.
    Requested,
}

impl ConnectionStreamRule {
    /// The rule's number in journal spec §9.8.
    pub fn number(self) -> u8 {
        match self {
            Self::Established => 66,
            Self::Rotated => 67,
            Self::AccountStream => 68,
            Self::Requested => 131,
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
    /// A range with no connection anchor reached a record rule 66, 67, 68, or 131 judges, so it
    /// fails closed there: cause `unanchored`, not a rule (journal spec v0.35 §11, DEC-885 item 5).
    Unanchored,
}

impl ConnectionCheck {
    /// The check's code as §11 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::LifecycleMismatch(_) | Self::Unanchored => "connection_lifecycle_mismatch",
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

/// §11's `connection_lifecycle_mismatch` over the full chain: the first row that breaks rule 66,
/// 67, 68, or 131.
pub fn verify_connection_lifecycle(rows: &[StoredEvent]) -> Result<(), ConnectionVerifyError> {
    Folds::default()
        .run(&connection_rows(rows))
        .map_or(Ok(()), |failing| Err(failing.indexed()))
}

/// Each stream's fold of rules 66, 67, 68, and 131, as the full-chain run holds it.
#[derive(Debug, Clone, Default)]
struct Folds {
    controls: BTreeMap<String, ControlFold>,
    accounts: BTreeMap<String, AccountFold>,
}

impl Folds {
    /// Continues each stream's fold over `rows`, stopping at the first that breaks a rule.
    fn run<'r>(&mut self, rows: &[Row<'r>]) -> Option<Failing<'r>> {
        rows.iter().find_map(|row| self.step(row))
    }

    /// Continues the fold of `row`'s stream over `row`, failing it when it breaks a rule.
    fn step<'r>(&mut self, row: &Row<'r>) -> Option<Failing<'r>> {
        let stream = row.stored.stream_id.clone();
        let refused = match row.stream {
            StreamType::Control => self.controls.entry(stream).or_default().admit(row).err(),
            _ => {
                let admitted = self.accounts.entry(stream).or_default().admits(row);
                (!admitted).then_some(ConnectionStreamRule::AccountStream)
            }
        };
        refused.map(|rule| row.failing(ConnectionCheck::LifecycleMismatch(rule)))
    }
}

/// The first failing row, by its index in the rows given and by the stored row itself.
struct Failing<'r> {
    index: usize,
    stored: &'r StoredEvent,
    check: ConnectionCheck,
}

impl Failing<'_> {
    fn indexed(&self) -> ConnectionVerifyError {
        mismatch(self.index, self.check)
    }

    fn located(&self) -> ConnectionCheckError {
        ConnectionCheckError::Failed(LocatedConnectionFailure {
            stream_id: self.stored.stream_id.clone(),
            seq: self.stored.seq,
            check: self.check,
        })
    }
}

/// Where a range's run of rules 66, 67, 68, and 131 starts (journal spec v0.35 §11, DEC-885).
#[derive(Debug, Clone)]
pub enum ConnectionStart {
    /// The rows start at each stream's `seq` 1: the full chain, anchored on nothing.
    Genesis,
    /// The rows continue the stored chain whose fold [`ConnectionAnchor::from_verified`] gave, one
    /// stream's (DEC-889).
    Anchored(ConnectionAnchor),
    /// The caller cannot read the chain before the range, or that chain breaks a rule: the run
    /// fails closed at the first record a rule judges.
    Unanchored,
}

/// The connection anchor (DEC-885 item 1): the state the full-chain fold of rules 66, 67, 68, and
/// 131 holds after each stream's records `1` to `from_seq − 1`, derived only by folding the stored
/// chain, never from a read model.
#[derive(Debug, Clone)]
pub struct ConnectionAnchor {
    scope: AnchorScope,
    folds: Folds,
}

/// The streams an anchored run judges.
#[derive(Debug, Clone)]
enum AnchorScope {
    /// Every stream the run is given, each from its own fold: the deprecated raw fold's, deleted
    /// with it (DEC-889 item 3).
    EveryStream,
    /// One stream's (DEC-889 item 1): the prefix's, or `None` for an empty prefix, whose stream is
    /// that of the range's first judged record (DEC-889 item 2).
    Stream(Option<String>),
}

impl ConnectionAnchor {
    /// The anchor after `prefix`, the stored rows of the range's streams from `seq` 1 up to its
    /// trusted start, in commit order; `None` when the prefix breaks rule 66, 67, 68, or 131, since
    /// a broken chain anchors nothing (DEC-885 item 2, I5). It trusts the rows unverified, so no
    /// caller may use it: [`ConnectionAnchor::from_verified`] replaces it (DEC-892), and it is
    /// deleted once that lands.
    #[deprecated(note = "unverified prefix; use from_verified (DEC-892)")]
    pub fn fold(prefix: &[StoredEvent]) -> Option<ConnectionAnchor> {
        Self::folded(prefix, AnchorScope::EveryStream)
    }

    /// The anchor of one stream after `prefix`, its rows `seq` 1 to `from_seq − 1` that
    /// [`VerifiedPrefix::bind`] verified from genesis and bound to the range's trusted start
    /// (DEC-892, DEC-889). `Broken` when the prefix breaks rule 66, 67, 68, or 131, since a broken
    /// chain anchors nothing (DEC-885 I5); the caller then runs the range
    /// [`ConnectionStart::Unanchored`], as it does when `bind` refuses the rows.
    pub fn from_verified(
        prefix: &VerifiedPrefix<'_>,
    ) -> Result<ConnectionAnchor, ConnectionAnchorError> {
        let rows = prefix.rows();
        let stream = rows.last().map(|row| row.stream_id.clone());
        Self::folded(rows, AnchorScope::Stream(stream)).ok_or(ConnectionAnchorError::Broken)
    }

    /// The anchor of `scope` after `rows`, or `None` when they break rule 66, 67, 68, or 131.
    fn folded(rows: &[StoredEvent], scope: AnchorScope) -> Option<ConnectionAnchor> {
        let mut folds = Folds::default();
        folds
            .run(&connection_rows(rows))
            .is_none()
            .then_some(ConnectionAnchor { scope, folds })
    }

    /// Continues the anchor's fold over `rows`. Scoped to one stream, it fails closed with
    /// [`ConnectionCheck::Unanchored`] at the first judged record of any other stream, and skips
    /// another stream's control-stream `ConnectionRevoked`, which nothing judges (DEC-889 item 2,
    /// DEC-885 I6).
    fn run<'r>(self, rows: &[Row<'r>]) -> Option<Failing<'r>> {
        let ConnectionAnchor { scope, mut folds } = self;
        let stream = match scope {
            AnchorScope::EveryStream => return folds.run(rows),
            AnchorScope::Stream(stream) => stream.or_else(|| {
                rows.iter()
                    .find(|row| row.judged())
                    .map(|row| row.stored.stream_id.clone())
            }),
        };
        rows.iter().find_map(|row| {
            if stream.as_ref() == Some(&row.stored.stream_id) {
                folds.step(row)
            } else {
                row.judged()
                    .then(|| row.failing(ConnectionCheck::Unanchored))
            }
        })
    }
}

/// Why [`ConnectionAnchor::from_verified`] gave no anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionAnchorError {
    /// The prefix breaks rule 66, 67, 68, or 131 (DEC-885 item 2, I5).
    Broken,
    /// Never returned now that E7-17 built the fold; kept, as `PrefixError` keeps its own, so a
    /// caller's match stays the same across the crate's stubs (DEC-77).
    Unimplemented { story: &'static str },
}

/// Why a located connection check did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionCheckError {
    /// The first row that fails, by its stream and `seq`.
    Failed(LocatedConnectionFailure),
}

/// The first row that fails a connection check, by its stream and `seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedConnectionFailure {
    pub stream_id: String,
    pub seq: u64,
    pub check: ConnectionCheck,
}

impl LocatedConnectionFailure {
    /// The check's code as §11 writes it.
    pub fn code(&self) -> &'static str {
        self.check.code()
    }
}

/// §11's `connection_lifecycle_mismatch` from `start` over `rows`, given as
/// [`verify_connection_lifecycle`] takes them. From [`ConnectionStart::Genesis`] it is the full
/// chain; anchored, it judges the anchor's stream as the full chain would, rule 131's closing
/// existence included, and fails closed with [`ConnectionCheck::Unanchored`] at the first record
/// of another stream that the unanchored run would fail at (DEC-889 item 2); unanchored, it
/// fails closed with [`ConnectionCheck::Unanchored`] at the first `ConnectionRequested`,
/// `ConnectionEstablished`, `ConnectionCredentialRotated`, or `ConnectionRefused` on a control
/// stream or connection record on an account stream, a `ConnectionRevoked` there included; a
/// control-stream `ConnectionRevoked` never fails (DEC-885 items 3 and 4, I6; DEC-888).
pub fn verify_connection_lifecycle_from(
    start: ConnectionStart,
    rows: &[StoredEvent],
) -> Result<(), ConnectionCheckError> {
    let rows = connection_rows(rows);
    let failing = match start {
        ConnectionStart::Genesis => Folds::default().run(&rows),
        ConnectionStart::Anchored(anchor) => anchor.run(&rows),
        ConnectionStart::Unanchored => rows
            .iter()
            .find(|row| row.judged())
            .map(|row| row.failing(ConnectionCheck::Unanchored)),
    };
    failing.map_or(Ok(()), |failing| Err(failing.located()))
}

/// [`verify_connection_causes`] over the full chain, reporting the failing row by its stream and
/// `seq`; the cause check has no range run (DEC-885 item 6).
pub fn verify_connection_causes_from_genesis(
    rows: &[StoredEvent],
) -> Result<(), ConnectionCheckError> {
    causes(&connection_rows(rows)).map_or(Ok(()), |failing| Err(failing.located()))
}

/// §11's `connection_cause_mismatch`: the first version-2 `ConnectionEstablished` or
/// `ConnectionCredentialRotated` on the control stream whose `causation_id` is not the passing
/// `ConnectionChecked` of its occasion (`connect`, `reconnect` for a second establishment, or
/// `reauthorize`) on the stream its `account_ref` names, for the same connection, listing
/// `contract` for an MCP broker; or the first copy on an account stream whose `causation_id`
/// names no control-stream original of its type, or whose payload without `risk_clock` differs
/// from that original's.
pub fn verify_connection_causes(rows: &[StoredEvent]) -> Result<(), ConnectionVerifyError> {
    causes(&connection_rows(rows)).map_or(Ok(()), |failing| Err(failing.indexed()))
}

fn causes<'r>(rows: &[Row<'r>]) -> Option<Failing<'r>> {
    let by_id: BTreeMap<&str, &Row<'_>> = rows
        .iter()
        .map(|row| (row.stored.event_id.as_str(), row))
        .collect();
    let mut connections: BTreeMap<(&str, &str), (&str, &str)> = BTreeMap::new();
    for row in rows {
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
            return Some(row.failing(ConnectionCheck::CauseMismatch));
        }
    }
    None
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

impl<'r> Row<'r> {
    fn failing(&self, check: ConnectionCheck) -> Failing<'r> {
        Failing {
            index: self.index,
            stored: self.stored,
            check,
        }
    }

    /// Whether a range rule judges the record: every connection record on an account stream, and
    /// a request, establishment, rotation, or refusal on a control stream (DEC-885 item 4, DEC-888).
    fn judged(&self) -> bool {
        self.stream != StreamType::Control
            || JUDGED_ON_CONTROL.contains(&self.stored.event_type.as_str())
    }

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

/// The connection manager's fold of one control stream (rules 66, 67, and 131).
#[derive(Debug, Clone, Default)]
struct ControlFold {
    /// Each connection's first establishment.
    first: BTreeMap<String, Value>,
    /// Each connection's latest `ConnectionEstablished` or `ConnectionRevoked`.
    latest: BTreeMap<String, &'static str>,
    /// Each connection's latest scopes, its establishment's or the latest rotation's.
    scopes: BTreeMap<String, BTreeSet<String>>,
    /// The connection each `account_ref` belongs to (CN-5).
    holders: BTreeMap<String, String>,
    /// Each connection's open `ConnectionRequested` (rule 131).
    open: BTreeMap<String, Value>,
    /// Every `account_ref` a `ConnectionRequested` named; rule 131 binds the stream once it holds one.
    requested: BTreeSet<String>,
    /// The connections whose latest revocation closed a request (rule 67's `reconnect` exception).
    withdrawn: BTreeSet<String>,
    /// The connections with a revocation that closed no request (rule 67's `connect` clause).
    plain: BTreeSet<String>,
}

impl ControlFold {
    fn admit(&mut self, row: &Row<'_>) -> Result<(), ConnectionStreamRule> {
        let p = row.payload();
        let id = text(p, "connection_id");
        let first = self.first.get(id).cloned();
        let latest = self.latest.get(id).copied();
        let same = |member: &str| {
            first
                .as_ref()
                .is_none_or(|f| text(f, member) == text(p, member))
        };
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
                let again = first.as_ref().is_some_and(|f| {
                    latest != Some(REVOKED)
                        || !same("broker")
                        || !same("environment")
                        || f.get("account_ref").and_then(Value::as_str).is_none()
                        || f.get("account_ref").and_then(Value::as_str) != account_ref
                });
                let held = account_ref.and_then(|r| self.holders.get(r));
                if again || held.is_some_and(|holder| holder != id) {
                    return Err(ConnectionStreamRule::Established);
                }
                let unrequested = match row.stored.schema_version {
                    _ if first.is_some() => false,
                    2 => self.unclosed(id, p, &ESTABLISHMENT_REPEATS),
                    _ => self.bound(),
                };
                if unrequested {
                    return Err(ConnectionStreamRule::Requested);
                }
                if let Some(account_ref) = account_ref {
                    self.holders
                        .entry(account_ref.to_owned())
                        .or_insert_with(|| id.to_owned());
                }
                self.first.entry(id.to_owned()).or_insert_with(|| p.clone());
                self.latest.insert(id.to_owned(), ESTABLISHED);
                self.scopes.insert(id.to_owned(), owned(&scopes));
            }
            REVOKED => {
                self.latest.insert(id.to_owned(), REVOKED);
                if self.open.remove(id).is_some() {
                    self.withdrawn.insert(id.to_owned());
                } else {
                    self.withdrawn.remove(id);
                    self.plain.insert(id.to_owned());
                }
            }
            ROTATED => {
                let narrower = self
                    .scopes
                    .get(id)
                    .is_some_and(|held| scopes.iter().all(|scope| held.contains(*scope)));
                if latest != Some(ESTABLISHED) || !narrower {
                    return Err(ConnectionStreamRule::Rotated);
                }
                self.scopes.insert(id.to_owned(), owned(&scopes));
            }
            REQUESTED => {
                let account_ref = text(p, "account_ref");
                if self.open.contains_key(id)
                    || first.is_some()
                    || self.requested.contains(account_ref)
                    || self.holders.contains_key(account_ref)
                {
                    return Err(ConnectionStreamRule::Requested);
                }
                self.open.insert(id.to_owned(), p.clone());
                self.requested.insert(account_ref.to_owned());
            }
            _ => {
                let occasion = text(p, "occasion");
                let fits = match occasion {
                    "connect" => first.is_none() && !self.plain.contains(id),
                    "reconnect" => latest == Some(REVOKED) && !self.withdrawn.contains(id),
                    "reauthorize" => latest == Some(ESTABLISHED),
                    _ => false,
                };
                if !fits || !same("broker") || !same("environment") {
                    return Err(ConnectionStreamRule::Rotated);
                }
                if occasion == "connect" && self.unclosed(id, p, &REFUSAL_REPEATS) {
                    return Err(ConnectionStreamRule::Requested);
                }
            }
        }
        Ok(())
    }

    /// Whether rule 131 binds the stream: it holds a `ConnectionRequested`.
    fn bound(&self) -> bool {
        !self.requested.is_empty()
    }

    /// Rule 131's closing clause on a connect's establishment or refusal `p` of `id`: whether it
    /// fails to close the id's open request once, repeating its `members`. With no open request it
    /// fails on a bound stream.
    fn unclosed(&mut self, id: &str, p: &Value, members: &[&str]) -> bool {
        match self.open.remove(id) {
            Some(request) => members
                .iter()
                .any(|member| p.get(member) != request.get(member)),
            None => self.bound(),
        }
    }
}

/// A record's scopes as a fold keeps them past the record.
fn owned(scopes: &BTreeSet<&str>) -> BTreeSet<String> {
    scopes.iter().map(|scope| (*scope).to_owned()).collect()
}

/// A check as an account stream's fold keeps it: whether it passed every result, whether it
/// listed `contract`, and the stream's latest entry into `suspended` before it.
#[derive(Debug, Clone, Copy)]
struct Check {
    passed: bool,
    contract: bool,
    suspension: Option<usize>,
}

/// The executor's fold of one account stream (rule 68).
#[derive(Debug, Clone, Default)]
struct AccountFold {
    /// The connection whose record came first; every later one names it.
    owner: Option<String>,
    /// The broker of the latest binding copy; `None` while the stream is `connecting`.
    broker: Option<String>,
    /// The latest check of each occasion.
    checks: BTreeMap<String, Check>,
    /// Each rotation's copy, by event id, with the stream's latest entry into `suspended` before
    /// its `reauthorize` check.
    rotations: BTreeMap<String, Option<usize>>,
    /// The state the latest `ConnectionStateChanged` left, `active` before any.
    state: Option<String>,
    /// Whether the latest `ConnectionStateChanged` was a `condition_cleared`.
    cleared: bool,
    /// How many times the stream has entered `suspended`, so an entry is told from every other
    /// across the full chain, a range's anchor included.
    entries: usize,
    /// The entry into `suspended` the stream last made, by its count.
    suspension: Option<usize>,
}

impl AccountFold {
    /// Whether rule 68 admits `row` on this stream; a `ConnectionRevoked` never, bound or not.
    fn admits(&mut self, row: &Row<'_>) -> bool {
        let p = row.payload();
        let id = text(p, "connection_id");
        if self.owner.get_or_insert_with(|| id.to_owned()) != id {
            return false;
        }
        let mcp = self
            .broker
            .as_deref()
            .is_some_and(|b| MCP_BROKERS.contains(&b));
        match row.stored.event_type.as_str() {
            CHECKED => {
                let occasion = text(p, "occasion");
                let contract = listed(p, "contract");
                let check = Check {
                    passed: all_passed(p),
                    contract,
                    suspension: self.suspension,
                };
                self.checks.insert(occasion.to_owned(), check);
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
                self.broker = Some(broker.to_owned());
                text(p, "account_ref") == stream_ref
                    && self.checks.get(occasion).is_some_and(|check| {
                        check.passed && (check.contract || !MCP_BROKERS.contains(&broker))
                    })
            }
            REVOKED => false,
            _ if self.broker.is_none() => false,
            ROTATED => match self.checks.get("reauthorize") {
                Some(check) if check.passed && (check.contract || !mcp) => {
                    self.rotations
                        .insert(row.stored.event_id.clone(), check.suspension);
                    true
                }
                _ => false,
            },
            "ConnectionStateChanged" => self.moves(row),
            _ => true,
        }
    }

    /// Rule 68's state clause on a `ConnectionStateChanged`.
    fn moves(&mut self, row: &Row<'_>) -> bool {
        let p = row.payload();
        let (from, to, reason) = (text(p, "from"), text(p, "to"), text(p, "reason"));
        let rotation = row.causation().and_then(|id| self.rotations.get(id));
        let admitted = from == self.state.as_deref().unwrap_or("active")
            && (reason != "acknowledged" || self.cleared)
            && (reason != "condition_cleared"
                || from != "suspended"
                || rotation.is_some_and(|suspension| *suspension == self.suspension));
        if to == "suspended" && from != "suspended" {
            self.entries = self.entries.saturating_add(1);
            self.suspension = Some(self.entries);
        }
        self.cleared = reason == "condition_cleared";
        self.state = Some(to.to_owned());
        admitted
    }
}
