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

use mandate_accounting::Position;
use mandate_canon::Value;
use mandate_gate_context::{Positions, Protection, RiskMark, Row};
use mandate_journal::StoredEvent;
use mandate_num::Usd;

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
    /// The `ApprovalRequested` version `content` comes from, which says which members it must
    /// hold: version 3's `deferred_to_binding` among them (DEC-2518).
    pub schema_version: u64,
    /// The `deferred_to_binding` of the `DecisionMade` the request's `causation_id` names on the
    /// agent stream, an empty list for a decision without one, or `null` when no such decision is
    /// there: what lets a reader catch a request that hides its decision's deferrals (DEC-2519).
    pub decision_deferred_to_binding: Value,
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
    let decisions = decision_deferrals(agent_rows);
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
                    schema_version: row.schema_version,
                    decision_deferred_to_binding: decided_for(&event, &decisions),
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

/// Each `DecisionMade` on the stream by event id, with its `payload.deferred_to_binding` as
/// journaled; an empty list when a version 1 or 2 decision names none, and `null` when a version 3
/// one, which always carries the member (rule 269), does not, so it is never read as agreeing
/// (DEC-2519).
fn decision_deferrals(agent_rows: &[StoredEvent]) -> std::collections::BTreeMap<String, Value> {
    agent_rows
        .iter()
        .filter(|row| row.event_type == "DecisionMade")
        .filter_map(|row| {
            let event = envelope(row)?;
            let named = event
                .get("payload")
                .and_then(|p| p.get("deferred_to_binding"))
                .cloned();
            let deferred = match named {
                Some(list) => list,
                None if row.schema_version >= 3 => Value::Null,
                None => Value::Array(Vec::new()),
            };
            Some((row.event_id.clone(), deferred))
        })
        .collect()
}

/// The deferred checks of the decision `event`'s `causation_id` names among `decisions`, or `null`
/// when it names none there (DEC-2519).
fn decided_for(event: &Value, decisions: &std::collections::BTreeMap<String, Value>) -> Value {
    text(event, "causation_id")
        .and_then(|id| decisions.get(id))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Whether `request`, an `ApprovalRequested` row among `agent_rows`, names exactly the deferred
/// checks of the decision it names as its cause: version 3's non-empty list, or none for versions 1
/// and 2, against the decision's list. A decision not on the stream, a version 3 decision without its
/// list, a list that is not distinct check names, and any other request disagree, so a grant of it
/// is refused (DEC-2521).
pub fn deferrals_agree(request: &StoredEvent, agent_rows: &[StoredEvent]) -> bool {
    let Some(event) = envelope(request) else {
        return false;
    };
    let named = event
        .get("payload")
        .and_then(|p| p.get("content"))
        .and_then(|c| c.get("deferred_to_binding"))
        .cloned();
    let requested = match (request.schema_version, named) {
        (3, Some(Value::Array(list))) if !list.is_empty() && check_names(&list) => {
            Value::Array(list)
        }
        (1 | 2, None) => Value::Array(Vec::new()),
        _ => return false,
    };
    decided_for(&event, &decision_deferrals(agent_rows)) == requested
}

/// Whether `list` holds distinct check names as the journal writes them, a lower-case letter then
/// lower-case letters, digits, and underscores, as the web app reads them (DEC-2519).
fn check_names(list: &[Value]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    list.iter().all(|item| {
        item.as_str().is_some_and(|name| {
            let mut chars = name.chars();
            chars.next().is_some_and(|first| first.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && seen.insert(name)
        })
    })
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
/// That row decides: one without an `account_ref`, or with one outside the id grammar
/// (`[A-Za-z0-9_-]+`), binds none rather than an older row's, so no stream name is formed from it.
pub fn account_ref(control: &[StoredEvent], connection_id: &str) -> Option<String> {
    let latest = |event_type: &str, schema_version: u64| {
        control.iter().rev().find_map(|row| {
            let event = envelope(row)?;
            (row.event_type == event_type
                && row.schema_version == schema_version
                && text(&event, "payload.connection_id") == Some(connection_id))
            .then_some(event)
        })
    };
    let event = latest("ConnectionEstablished", 2).or_else(|| latest("ConnectionRequested", 1))?;
    text(&event, "payload.account_ref")
        .filter(|reference| {
            !reference.is_empty()
                && reference
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .map(str::to_owned)
}

/// The account's holdings at its last reconciliation (DEC-1020 item 3): the latest
/// `BrokerPositionObserved` per instrument, in instrument order, leaving out an instrument whose
/// model and broker quantities are both zero.
pub fn account_positions(account_rows: &[StoredEvent]) -> Vec<Value> {
    let mut fold = HoldingsFold::default();
    for row in account_rows {
        fold.fold(row);
    }
    fold.finish()
}

/// A fold of an account stream's rows, one row at a time, so a reader can resume it from the last
/// row it folded (DEC-2145 item 10).
pub trait AccountFold: Clone + Send {
    /// What the fold reads once its rows are folded.
    type Out;
    /// Folds one more row; `None` when the row cannot be read and nothing is served.
    fn fold(&mut self, row: &StoredEvent) -> Option<()>;
    /// What the rows folded so far read as.
    fn finish(&self) -> Self::Out;
}

/// [`account_positions`]'s fold: the latest `BrokerPositionObserved` per instrument.
#[derive(Debug, Clone, Default)]
pub struct HoldingsFold {
    latest: std::collections::BTreeMap<String, Value>,
}

impl AccountFold for HoldingsFold {
    type Out = Vec<Value>;

    fn fold(&mut self, row: &StoredEvent) -> Option<()> {
        if row.event_type != "BrokerPositionObserved" {
            return Some(());
        }
        let Some(event) = envelope(row) else {
            return Some(());
        };
        let member = |name: &str| event.get("payload").and_then(|p| p.get(name)).cloned();
        let (Some(Value::Str(instrument)), Some(qty), Some(broker), Some(mismatch)) = (
            member("instrument"),
            member("model_qty"),
            member("broker_qty"),
            member("mismatch"),
        ) else {
            return Some(());
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
                self.latest.insert(instrument, position);
            }
            _ => {
                self.latest.remove(&instrument);
            }
        }
        Some(())
    }

    fn finish(&self) -> Vec<Value> {
        self.latest.values().cloned().collect()
    }
}

/// The agent's orders on its account stream (`read-models/orders`, DEC-2111 item 3), in the order
/// first submitted: each `client_order_id` whose `OrderSubmitted` names, as its `causation_id`, an
/// `OrderRequestRecorded` for `agent_id` (journal spec §9.5 rule 45). The request is the first
/// `OrderSubmitted`'s and its companion's; `state` and `filled_qty` are the last
/// `OrderStateChanged`'s, `null` before any; `updated_at` is the order's last record's
/// `recorded_at`. An order no companion names is no agent's, and is never listed.
pub fn agent_orders(account: &[StoredEvent], agent_id: &str) -> Vec<Value> {
    let mut fold = OrdersFold::new(agent_id);
    for row in account {
        fold.fold(row);
    }
    fold.finish()
}

/// [`agent_orders`]'s fold for one agent: its companions by event id, and its orders in the
/// order first submitted.
#[derive(Debug, Clone)]
pub struct OrdersFold {
    agent_id: String,
    companions: std::collections::BTreeMap<String, Value>,
    orders: Vec<(String, Vec<(&'static str, Value)>)>,
}

impl OrdersFold {
    /// The fold of no rows for `agent_id`.
    pub fn new(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_owned(),
            companions: std::collections::BTreeMap::new(),
            orders: Vec::new(),
        }
    }
}

impl AccountFold for OrdersFold {
    type Out = Vec<Value>;

    fn fold(&mut self, row: &StoredEvent) -> Option<()> {
        let Some(event) = envelope(row) else {
            return Some(());
        };
        let payload = event.get("payload").cloned().unwrap_or(Value::Null);
        let member = |name: &str| payload.get(name).cloned().unwrap_or(Value::Null);
        let order_id = text(&event, "payload.client_order_id").map(str::to_owned);
        let known = order_id
            .as_deref()
            .and_then(|id| self.orders.iter().position(|(known, _)| known == id));
        let recorded_at = Value::Str(row.recorded_at.clone());
        match (row.event_type.as_str(), known) {
            ("OrderRequestRecorded", _)
                if text(&event, "payload.agent_id") == Some(self.agent_id.as_str()) =>
            {
                self.companions
                    .insert(row.event_id.clone(), payload.clone());
            }
            ("OrderSubmitted" | "OrderStateChanged", Some(at)) => {
                if let Some((_, members)) = self.orders.get_mut(at) {
                    if row.event_type == "OrderStateChanged" {
                        set(members, "state", member("state"));
                        set(members, "filled_qty", member("filled_qty"));
                    }
                    set(members, "updated_at", recorded_at);
                }
            }
            ("OrderSubmitted", None) => {
                let companion = text(&event, "causation_id").and_then(|id| self.companions.get(id));
                if let (Some(id), Some(companion)) = (order_id, companion) {
                    let request = |name: &str| companion.get(name).cloned().unwrap_or(Value::Null);
                    let members = vec![
                        ("client_order_id", Value::Str(id.clone())),
                        ("intent_id", request("intent_id")),
                        ("purpose", request("purpose")),
                        ("instrument_id", member("instrument_id")),
                        ("side", member("side")),
                        ("type", member("type")),
                        ("tif", member("tif")),
                        ("qty", member("qty")),
                        ("limit_price", member("limit_price")),
                        ("stop_price", request("stop_price")),
                        ("state", Value::Null),
                        ("filled_qty", Value::Null),
                        ("submitted_at", recorded_at.clone()),
                        ("updated_at", recorded_at),
                    ];
                    self.orders.push((id, members));
                }
            }
            _ => {}
        }
        Some(())
    }

    fn finish(&self) -> Vec<Value> {
        self.orders
            .iter()
            .filter_map(|(_, members)| object(members.clone()))
            .collect()
    }
}

/// Replaces `name`'s value in `members`.
fn set(members: &mut [(&'static str, Value)], name: &str, value: Value) {
    if let Some((_, held)) = members.iter_mut().find(|(key, _)| *key == name) {
        *held = value;
    }
}

/// The agent's net filled quantity per instrument (`read-models/positions`, DEC-2111 item 4): the
/// `FillApplied` and `LateFillApplied` records of its own orders, buys adding and sells
/// subtracting, in instrument order, leaving out an instrument that nets to zero. Its orders are
/// those `mandate_gate_context::Positions` attributes to it (DEC-2138 item 1): its requests, their
/// replacements, and the protective legs the broker created for its entries. Beside it, DEC-2138's
/// figures from the same reading: the sub-ledger's §8.1 `cost_basis`, the latest counted
/// `risk_mark`, §8.2's `market_value` and `unrealized_pnl` at that mark when it is sane and the
/// position long, and the account's `protection`. `None` when a row the reading reads cannot be
/// read, so no position is served from part of the journal.
pub fn agent_positions(account: &[StoredEvent], agent_id: &str) -> Option<Vec<Value>> {
    let mut fold = PositionsFold::new(agent_id);
    for row in account {
        fold.fold(row)?;
    }
    fold.finish()
}

/// [`agent_positions`]'s fold for one agent: `mandate_gate_context::Positions` over its account
/// stream, begun at the stream's first row.
#[derive(Debug, Clone)]
pub struct PositionsFold {
    agent_id: String,
    read: Option<Positions>,
}

impl PositionsFold {
    /// The fold of no rows for `agent_id`.
    pub fn new(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_owned(),
            read: None,
        }
    }
}

impl AccountFold for PositionsFold {
    type Out = Option<Vec<Value>>;

    fn fold(&mut self, row: &StoredEvent) -> Option<()> {
        let agent_id = &self.agent_id;
        let read = self
            .read
            .get_or_insert_with(|| Positions::new(&row.stream_id, agent_id));
        let event = envelope(row)?;
        read.fold(Row {
            stream: &row.stream_id,
            event_type: &row.event_type,
            payload: event.get("payload")?,
        })
        .ok()
    }

    fn finish(&self) -> Option<Vec<Value>> {
        let Some(read) = &self.read else {
            return Some(Vec::new());
        };
        read.filled()
            .map(|(instrument, qty)| {
                let basis = read.cost_basis(instrument);
                let mark = read.risk_mark(instrument);
                let valued = valued(basis, mark)?;
                object(vec![
                    ("instrument_id", Value::Str(instrument.to_owned())),
                    ("filled_qty", Value::Str(qty.to_string())),
                    ("cost_basis", basis.map_or(Some(Value::Null), cost_basis)?),
                    ("risk_mark", mark.map_or(Some(Value::Null), risk_mark)?),
                    ("market_value", decimal(valued.map(|(value, _)| value))),
                    ("unrealized_pnl", decimal(valued.map(|(_, pnl)| pnl))),
                    (
                        "protection",
                        protection_value(&read.protection(instrument))?,
                    ),
                ])
            })
            .collect()
    }
}

/// A decimal figure, or `null` when it is not served.
fn decimal(figure: Option<impl std::fmt::Display>) -> Value {
    figure.map_or(Value::Null, |figure| Value::Str(figure.to_string()))
}

/// §8.2's market value Q × bid and unrealized P&L Q × bid − B, exactly: `Some(None)` unless the
/// basis is served, long or flat, and the mark is sane (DEC-2138 item 3; a short is marked at the
/// ask, which a risk mark does not carry), `None` when the arithmetic overflows.
fn valued(basis: Option<Position>, mark: Option<RiskMark>) -> Option<Option<(Usd, Usd)>> {
    let long = basis.filter(|basis| !basis.qty().is_negative());
    let (Some(basis), Some(bid)) = (long, mark.filter(|mark| mark.sane).and_then(|m| m.bid)) else {
        return Some(None);
    };
    let value = basis.qty().value_at(bid).ok()?;
    let pnl = value.checked_sub(basis.basis().to_usd()).ok()?;
    Some(Some((value, pnl)))
}

fn cost_basis(position: Position) -> Option<Value> {
    object(vec![
        ("qty", Value::Str(position.qty().to_string())),
        ("basis", Value::Str(position.basis().to_string())),
    ])
}

fn risk_mark(mark: RiskMark) -> Option<Value> {
    object(vec![
        ("bid", decimal(mark.bid)),
        ("sane", Value::Bool(mark.sane)),
        ("session", Value::Str(mark.session.as_str().to_owned())),
        ("marked_at", Value::Str(mark.at.to_string())),
    ])
}

fn protection_value(protection: &Protection) -> Option<Value> {
    object(vec![
        (
            "orders",
            Value::Array(protection.orders.iter().cloned().map(Value::Str).collect()),
        ),
        (
            "covered_qty",
            Value::Str(protection.covered_qty.to_string()),
        ),
        ("stop", decimal(protection.stop)),
        ("take_profit", decimal(protection.take_profit)),
        ("unprotected_since", decimal(protection.unprotected_since)),
    ])
}
