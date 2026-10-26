//! The alerts read (`read-models/alerts`; workspace API spec §4.7; DEC-2141), folded from journal
//! rows alone (API-14).
//!
//! - **Streams.** The control stream names the streams an alert may be on, as the dispatcher reads
//!   it (DEC-2516 item 1): every agent `AgentDeployed` names, every account `ConnectionRequested`
//!   or `ConnectionEstablished` names. [`followed`] lists them; the route reads each.
//! - **Alerts.** Each `OwnerAlertSent` whose kind notifications spec §3.2 sends to an agent's or an
//!   account's owners is one alert, most recently raised first. The nine kinds about members and
//!   access ([`WITHHELD`]) are never served. An alert that does not read, names any other kind, or
//!   names a subject its own stream does not hold fails the whole read: never a shorter list
//!   (`AGENTS.md` rule 3).
//! - **Notices.** An alert's notice is the `NoticeIssued` whose `cause` is it, with the asking
//!   member's own latest `NoticeAttempted` per channel, the one with the highest notice-stream
//!   `seq`, listed in channel order. Another member's attempts are never read into a view.
//!
//! Every function here is pure: rows in, values out.

use std::collections::BTreeMap;

use mandate_canon::Value;
use mandate_journal::StoredEvent;

use crate::reads::envelope;

/// The kinds about members and access, which notifications spec §3.2 sends to one member or to
/// admins alone, and this read never serves (DEC-2141 item 2).
pub const WITHHELD: [&str; 9] = [
    "credential_added",
    "new_device",
    "notification_address_changed",
    "recovery_used",
    "role_granted",
    "member_deactivated",
    "deprovisioned",
    "break_glass",
    "client_connected",
];

/// The kinds this read serves, each with its class from notifications spec §3.2.
const SERVED: [(&str, &str); 23] = [
    ("risk_limit", "safety"),
    ("kill_switch", "safety"),
    ("kill_switch_unapplied", "safety"),
    ("agent_held", "safety"),
    ("retirement_held", "safety"),
    ("account_restriction", "safety"),
    ("protection", "safety"),
    ("exit_stalled", "safety"),
    ("reconciliation", "safety"),
    ("external_activity", "safety"),
    ("account_state", "safety"),
    ("data_feed_down", "safety"),
    ("integrity_incident", "safety"),
    ("version_risk_increasing", "safety"),
    ("delegation_added", "safety"),
    ("connection_added", "safety"),
    ("went_live", "safety"),
    ("daily_brief", "info"),
    ("delegation_ended", "info"),
    ("model_status", "info"),
    ("research_status", "info"),
    ("spend_cap", "info"),
    ("approval_closed", "info"),
];

/// The class of a kind this read serves; `None` for a withheld kind or one no alert takes.
pub fn class(kind: &str) -> Option<&'static str> {
    SERVED
        .iter()
        .find(|(served, _)| *served == kind)
        .map(|(_, class)| *class)
}

/// The recipient id notifications spec §5.9 gives the member with ULID `member`: `p_` and the ULID
/// in lowercase.
pub fn recipient(member: &str) -> String {
    format!("p_{}", member.to_ascii_lowercase())
}

/// Which stream an alert is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Followed {
    Control,
    Agent(String),
    Account(String),
}

/// One followed stream and its rows.
#[derive(Debug, Clone)]
pub struct Stream<'a> {
    pub followed: Followed,
    pub rows: &'a [StoredEvent],
}

/// The member's latest attempt on one channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryView {
    pub channel: String,
    pub status: String,
    pub attempt: u64,
    pub recorded_at: String,
}

/// An alert's notice, with the member's own deliveries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeView {
    pub issued_at: String,
    pub delivery: Vec<DeliveryView>,
}

/// One alert, as `read-models/alerts` states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertView {
    pub alert_id: String,
    pub kind: String,
    pub class: &'static str,
    pub raised_at: String,
    pub followed: Followed,
    pub subject_id: String,
    pub subject_type: String,
    pub owner_command: Option<String>,
    pub notice: Option<NoticeView>,
}

/// A record the read cannot serve without dropping it: the read fails (`journal_unavailable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// The text at `key` in `payload`.
fn member<'v>(payload: &'v Value, key: &str) -> Option<&'v str> {
    payload.get(key).and_then(Value::as_str)
}

/// A row's payload; `None` for a body that does not read.
fn payload(row: &StoredEvent) -> Option<Value> {
    envelope(row).and_then(|event| event.get("payload").cloned())
}

/// The agents and accounts `control` names, each once, in first-named order.
pub fn followed(control: &[StoredEvent]) -> (Vec<String>, Vec<String>) {
    let mut agents: Vec<String> = Vec::new();
    let mut accounts: Vec<String> = Vec::new();
    for row in control {
        let (list, key) = match row.event_type.as_str() {
            "AgentDeployed" => (&mut agents, "agent_id"),
            "ConnectionRequested" | "ConnectionEstablished" => (&mut accounts, "account_ref"),
            _ => continue,
        };
        let Some(named) = payload(row).and_then(|p| member(&p, key).map(str::to_owned)) else {
            continue;
        };
        if !list.contains(&named) {
            list.push(named);
        }
    }
    (agents, accounts)
}

/// The member's own deliveries, keyed by notice id, from the notice stream's rows.
fn deliveries(
    notices: &[StoredEvent],
    recipient: &str,
) -> Result<BTreeMap<String, BTreeMap<String, DeliveryView>>, Unreadable> {
    let mut latest: BTreeMap<String, BTreeMap<String, (u64, DeliveryView)>> = BTreeMap::new();
    for row in notices
        .iter()
        .filter(|row| row.event_type == "NoticeAttempted")
    {
        let attempt = payload(row).ok_or(Unreadable)?;
        if member(&attempt, "recipient") != Some(recipient) {
            continue;
        }
        let (Some(notice), Some(channel), Some(status), Some(number)) = (
            member(&attempt, "notice"),
            member(&attempt, "channel"),
            member(&attempt, "status"),
            attempt.get("attempt").and_then(Value::as_int),
        ) else {
            return Err(Unreadable);
        };
        let view = DeliveryView {
            channel: channel.to_owned(),
            status: status.to_owned(),
            attempt: number,
            recorded_at: row.recorded_at.clone(),
        };
        let channels = latest.entry(notice.to_owned()).or_default();
        if channels.get(channel).is_none_or(|(seq, _)| *seq < row.seq) {
            channels.insert(channel.to_owned(), (row.seq, view));
        }
    }
    Ok(latest
        .into_iter()
        .map(|(notice, channels)| {
            (
                notice,
                channels
                    .into_iter()
                    .map(|(channel, (_, view))| (channel, view))
                    .collect(),
            )
        })
        .collect())
}

/// Each issued notice by its cause: its id and `issued_at`.
fn issued(notices: &[StoredEvent]) -> Result<BTreeMap<String, (String, String)>, Unreadable> {
    let mut by_cause = BTreeMap::new();
    for row in notices
        .iter()
        .filter(|row| row.event_type == "NoticeIssued")
    {
        let notice = payload(row).ok_or(Unreadable)?;
        let (Some(cause), Some(id)) = (member(&notice, "cause"), member(&notice, "notice")) else {
            return Err(Unreadable);
        };
        by_cause
            .entry(cause.to_owned())
            .or_insert((id.to_owned(), row.recorded_at.clone()));
    }
    Ok(by_cause)
}

/// One alert from its row on `stream`, or `None` for a withheld kind.
fn alert(stream: &Stream<'_>, row: &StoredEvent) -> Result<Option<AlertView>, Unreadable> {
    let raised = payload(row).ok_or(Unreadable)?;
    let kind = member(&raised, "kind").ok_or(Unreadable)?;
    if WITHHELD.contains(&kind) {
        return Ok(None);
    }
    let class = class(kind).ok_or(Unreadable)?;
    let subject_id = member(&raised, "subject").ok_or(Unreadable)?;
    let subject = stream
        .rows
        .iter()
        .find(|candidate| candidate.event_id == subject_id)
        .ok_or(Unreadable)?;
    let owner_command = match raised.get("owner_command") {
        None | Some(Value::Null) => None,
        Some(Value::Str(command)) => Some(command.clone()),
        Some(_) => return Err(Unreadable),
    };
    Ok(Some(AlertView {
        alert_id: row.event_id.clone(),
        kind: kind.to_owned(),
        class,
        raised_at: row.recorded_at.clone(),
        followed: stream.followed.clone(),
        subject_id: subject_id.to_owned(),
        subject_type: subject.event_type.clone(),
        owner_command,
        notice: None,
    }))
}

/// Every alert on `streams`, most recently raised first (`raised_at`, then `alert_id`, both
/// descending), each with its notice and `recipient`'s own deliveries from `notices`.
///
/// # Errors
/// [`Unreadable`] for an alert, a notice, or an attempt that does not read, a kind no alert takes,
/// or a subject its stream does not hold.
pub fn alerts(
    streams: &[Stream<'_>],
    notices: &[StoredEvent],
    recipient: &str,
) -> Result<Vec<AlertView>, Unreadable> {
    let issued = issued(notices)?;
    let mut delivered = deliveries(notices, recipient)?;
    let mut views = Vec::new();
    for stream in streams {
        for row in stream
            .rows
            .iter()
            .filter(|row| row.event_type == "OwnerAlertSent")
        {
            let Some(mut view) = alert(stream, row)? else {
                continue;
            };
            view.notice = issued
                .get(&view.alert_id)
                .map(|(id, issued_at)| NoticeView {
                    issued_at: issued_at.clone(),
                    delivery: delivered
                        .remove(id)
                        .map(|channels| channels.into_values().collect())
                        .unwrap_or_default(),
                });
            views.push(view);
        }
    }
    views.sort_by(|a, b| {
        b.raised_at
            .cmp(&a.raised_at)
            .then_with(|| b.alert_id.cmp(&a.alert_id))
    });
    Ok(views)
}
