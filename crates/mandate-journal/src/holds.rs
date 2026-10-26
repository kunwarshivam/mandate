//! The holds an owner can acknowledge (journal spec §9.28, DEC-2830, DEC-2144), folded once for
//! every reader: the workspace API's list, `mandate agent holds`, and, by a differential test, the
//! executor's own checks 1 and 2, so the three never disagree about what stands.
//!
//! [`standing`] folds one account stream to the holds that stand at its end:
//!
//! - a `RiskLimitTriggered` stands until a later trigger or `RiskLimitLifted` of the same agent's
//!   `limit`, or an `OwnerAcknowledged` copy naming a standing trigger of it, unless the very next
//!   record is that copy's `OwnerCommandRefused`, which restores what it ended (the executor's
//!   reading, `mandate-executor`'s `acknowledge.rs`);
//! - an `AccountRestrictionChanged` stands until a later change or `AccountRestrictionLifted` of
//!   its `cause` (DEC-2824);
//! - the executor's own `AgentModeApplied` (`originated`) to `exits_only` or `paused` under
//!   `reconciliation:<subject>` stands while it is the latest `AgentModeApplied` of its agent (or
//!   `*`) and restriction (DEC-2830).
//!
//! A record that raises or ends a hold and does not read fails the fold, so no reader leaves a hold
//! out or serves one already ended. [`requests`] and [`independence_required`] read the control
//! stream: the requests still unanswered, and whether the workspace's policy requires an
//! independent user.

use std::collections::BTreeMap;

use mandate_canon::{Value, parse};

use crate::StoredEvent;

/// A record a fold must read did not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// The prefix of a reconciliation hold's `restriction` (`mandate-executor`'s `restriction_for`).
const RECONCILIATION: &str = "reconciliation:";

/// The reconciliation subjects that are not an instrument (trading spec §7.1 and §11).
const ACCOUNT_KINDS: [&str; 4] = ["cash", "fees", "external_activity", "incomplete"];

/// The membership records that carry the workspace's effective `independent_approval_required` at
/// their instant (DEC-648 item 7).
const MEMBERSHIP: [&str; 3] = ["MemberActivated", "MemberRoleChanged", "MemberReactivated"];

/// What a hold holds, by `subject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A latched limit, rung, or tripwire: its `limit` code.
    Limit(String),
    /// An account restriction: its `cause`.
    Cause(String),
    /// A reconciliation hold: its `restriction`, its kind, and the instrument it holds.
    Reconciliation {
        restriction: String,
        kind: String,
        instrument: Option<String>,
    },
}

impl Reason {
    /// §9.28 version 2's `subject` for it.
    #[must_use]
    pub const fn subject(&self) -> &'static str {
        match self {
            Self::Limit(_) => "limit",
            Self::Cause(_) => "restriction",
            Self::Reconciliation { .. } => "reconciliation",
        }
    }
}

/// Whose a hold is: one agent's, or the account's (a restriction, or a reconciliation hold of
/// every agent, `*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Agent(String),
    Account,
}

/// One hold standing on an account stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The `event_id` of the record that raised it: the `event` an acknowledgment names.
    pub event: String,
    pub holder: Holder,
    pub reason: Reason,
    /// The agent mode it holds, `None` for a limit.
    pub mode: Option<&'static str>,
    /// The raising record's `recorded_at`.
    pub raised_at: String,
}

/// One row's members the folds read.
struct Read {
    payload: Value,
    causation: Option<String>,
}

fn read(row: &StoredEvent) -> Result<Read, Unreadable> {
    let body = parse(&row.body).map_err(|_| Unreadable)?;
    let payload = body.get("payload").cloned().ok_or(Unreadable)?;
    let causation = body
        .get("causation_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(Read { payload, causation })
}

fn text(payload: &Value, member: &str) -> Result<String, Unreadable> {
    payload
        .get(member)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(Unreadable)
}

/// An acknowledgment copy the next record may refuse, and the holds it ended, by their order.
struct Pending {
    original: String,
    ended: Vec<(usize, Raised)>,
}

/// The holds that stand at the end of one account stream's `rows`, in the order raised.
///
/// # Errors
/// [`Unreadable`] when a record that raises or ends a hold, an acknowledgment copy or a refusal
/// following one does not read.
pub fn standing(rows: &[StoredEvent]) -> Result<Vec<Raised>, Unreadable> {
    let mut held: BTreeMap<usize, Raised> = BTreeMap::new();
    let mut pending: Option<Pending> = None;
    for (order, row) in rows.iter().enumerate() {
        let kind = row.event_type.as_str();
        if let Some(Pending { original, ended }) = pending.take()
            && kind == "OwnerCommandRefused"
        {
            let refusal = read(row)?;
            let refuses = refusal.payload.get("command").and_then(Value::as_str)
                == Some("acknowledge")
                && refusal.causation.as_deref() == Some(original.as_str());
            if refuses {
                held.extend(ended);
            }
        }
        match kind {
            "RiskLimitTriggered" | "RiskLimitLifted" => {
                let payload = read(row)?.payload;
                let (agent, limit) = (text(&payload, "agent_id")?, text(&payload, "limit")?);
                end_limit(&mut held, &agent, &limit);
                if kind == "RiskLimitTriggered" {
                    held.insert(
                        order,
                        Raised {
                            event: row.event_id.clone(),
                            holder: Holder::Agent(agent),
                            reason: Reason::Limit(limit),
                            mode: None,
                            raised_at: row.recorded_at.clone(),
                        },
                    );
                }
            }
            "OwnerAcknowledged" => {
                let copy = read(row)?;
                let original = copy.causation.ok_or(Unreadable)?;
                let named = text(&copy.payload, "event")?;
                let trigger = held
                    .values()
                    .find_map(|hold| match (&hold.holder, &hold.reason) {
                        (Holder::Agent(agent), Reason::Limit(limit)) if hold.event == named => {
                            Some((agent.clone(), limit.clone()))
                        }
                        _ => None,
                    });
                let ended = trigger
                    .map(|(agent, limit)| end_limit(&mut held, &agent, &limit))
                    .unwrap_or_default();
                pending = Some(Pending { original, ended });
            }
            "AccountRestrictionChanged" | "AccountRestrictionLifted" => {
                let payload = read(row)?.payload;
                let cause = text(&payload, "cause")?;
                held.retain(|_, hold| hold.reason != Reason::Cause(cause.clone()));
                if kind == "AccountRestrictionChanged" {
                    let mode = match text(&payload, "restriction")?.as_str() {
                        "closing_only" => "exits_only",
                        "blocked" => "paused",
                        _ => return Err(Unreadable),
                    };
                    held.insert(
                        order,
                        Raised {
                            event: row.event_id.clone(),
                            holder: Holder::Account,
                            reason: Reason::Cause(cause),
                            mode: Some(mode),
                            raised_at: row.recorded_at.clone(),
                        },
                    );
                }
            }
            "AgentModeApplied" => {
                let payload = read(row)?.payload;
                let restriction = text(&payload, "restriction")?;
                let Some(subject) = restriction.strip_prefix(RECONCILIATION) else {
                    continue;
                };
                let agent = text(&payload, "agent")?;
                let holder = if agent == "*" {
                    Holder::Account
                } else {
                    Holder::Agent(agent)
                };
                held.retain(|_, hold| {
                    !(hold.holder == holder
                        && matches!(&hold.reason, Reason::Reconciliation { restriction: r, .. } if *r == restriction))
                });
                let Some(Value::Bool(originated)) = payload.get("originated") else {
                    return Err(Unreadable);
                };
                let mode = match text(&payload, "to")?.as_str() {
                    "exits_only" => "exits_only",
                    "paused" => "paused",
                    _ => continue,
                };
                if !originated {
                    continue;
                }
                let account_wide = ACCOUNT_KINDS.contains(&subject);
                let reason = Reason::Reconciliation {
                    kind: if account_wide { subject } else { "instrument" }.to_owned(),
                    instrument: (!account_wide).then(|| subject.to_owned()),
                    restriction: restriction.clone(),
                };
                held.insert(
                    order,
                    Raised {
                        event: row.event_id.clone(),
                        holder,
                        reason,
                        mode: Some(mode),
                        raised_at: row.recorded_at.clone(),
                    },
                );
            }
            _ => {}
        }
    }
    Ok(held.into_values().collect())
}

/// Ends every standing trigger of `agent`'s `limit`, answering what it ended, by order.
fn end_limit(held: &mut BTreeMap<usize, Raised>, agent: &str, limit: &str) -> Vec<(usize, Raised)> {
    let (ended, kept): (BTreeMap<usize, Raised>, BTreeMap<usize, Raised>) =
        std::mem::take(held).into_iter().partition(|(_, hold)| {
            hold.holder == Holder::Agent(agent.to_owned())
                && hold.reason == Reason::Limit(limit.to_owned())
        });
    *held = kept;
    ended.into_iter().collect()
}

/// An `OwnerAcknowledgmentRequested` on the control stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requested {
    pub event_id: String,
    pub agent: String,
    pub event: String,
    /// Its subject, `limit` for a version-1 request, and empty for a `subject` that is not text,
    /// which matches no hold (DEC-2838).
    pub subject: String,
    pub requested_by: String,
}

/// Every `OwnerAcknowledgmentRequested` on `control`, in stream order, with whether an
/// `OwnerAcknowledged` names it (rule 241).
///
/// # Errors
/// [`Unreadable`] when a request or an acknowledgment does not read.
pub fn requests(control: &[StoredEvent]) -> Result<Vec<(Requested, bool)>, Unreadable> {
    let mut spent: Vec<String> = Vec::new();
    for row in control
        .iter()
        .filter(|r| r.event_type == "OwnerAcknowledged")
    {
        if let Some(request) = read(row)?.payload.get("request").and_then(Value::as_str) {
            spent.push(request.to_owned());
        }
    }
    let mut found = Vec::new();
    for row in control
        .iter()
        .filter(|r| r.event_type == "OwnerAcknowledgmentRequested")
    {
        let payload = read(row)?.payload;
        let subject = match (row.schema_version, payload.get("subject")) {
            (1, None) => "limit".to_owned(),
            (_, Some(Value::Str(subject))) => subject.clone(),
            _ => String::new(),
        };
        let requested = Requested {
            event_id: row.event_id.clone(),
            agent: text(&payload, "agent")?,
            event: text(&payload, "event")?,
            subject,
            requested_by: text(&payload, "requested_by")?,
        };
        let is_spent = spent.contains(&requested.event_id);
        found.push((requested, is_spent));
    }
    Ok(found)
}

/// Whether the workspace's policy requires an independent user to acknowledge, from `control`'s
/// rows of one environment: the latest membership record's `independent_approval_required`,
/// `false` with none, and `true` while that record does not read, never `false` (the executor
/// process's overlay, DEC-2830; no effective-policy fold exists yet, backlog E9-6).
#[must_use]
pub fn independence_required(control: &[StoredEvent]) -> bool {
    control
        .iter()
        .rfind(|row| MEMBERSHIP.contains(&row.event_type.as_str()))
        .is_some_and(|row| {
            read(row)
                .ok()
                .and_then(|r| r.payload.get("independent_approval_required").cloned())
                != Some(Value::Bool(false))
        })
}

/// One hold as a list serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub raised: Raised,
    /// The agent to post an acknowledgment to.
    pub agent_id: String,
    /// The account stream's id.
    pub account: String,
    /// The latest unspent request for it.
    pub request: Option<Requested>,
}

impl Hold {
    /// Whether it blocks `agent`, whose account stream is `account`: its own limit or hold, or a
    /// hold of every agent of that account.
    #[must_use]
    pub fn blocks(&self, agent: &str, account: &str) -> bool {
        self.account == account
            && match &self.raised.holder {
                Holder::Agent(holder) => holder == agent,
                Holder::Account => true,
            }
    }
}

/// The holds standing on each account in `accounts` (its stream id, its rows, and its deployed
/// agents in id order), each with the agent to post to and its latest unspent request of the same
/// event and subject. A hold of the whole account with no request and no agent is left out, as no
/// agent can acknowledge it.
///
/// # Errors
/// [`standing`]'s.
pub fn holds(
    accounts: &[(String, Vec<StoredEvent>, Vec<String>)],
    requested: &[(Requested, bool)],
) -> Result<Vec<Hold>, Unreadable> {
    let mut listed = Vec::new();
    for (account, rows, agents) in accounts {
        for raised in standing(rows)? {
            let request = requested
                .iter()
                .filter(|(r, spent)| {
                    !spent && r.event == raised.event && r.subject == raised.reason.subject()
                })
                .map(|(r, _)| r.clone())
                .next_back();
            let agent_id = match (&request, &raised.holder) {
                (Some(request), _) => request.agent.clone(),
                (None, Holder::Agent(agent)) => agent.clone(),
                (None, Holder::Account) => match agents.first() {
                    Some(agent) => agent.clone(),
                    None => continue,
                },
            };
            listed.push(Hold {
                raised,
                agent_id,
                account: account.clone(),
                request,
            });
        }
    }
    Ok(listed)
}
