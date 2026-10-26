//! The mandate's approver references resolved to the user ids admission compares (mandate spec
//! §6.4 check 3, V-024; [DEC-953](../../../docs/project/decisions/DEC-953.md),
//! [DEC-2384](../../../docs/project/decisions/DEC-2384.md)).
//!
//! A mandate names its approvers as `role:approver`, `role:owner`, `role:workspace_admin` or
//! `user:<id>`, while an answer names its responder by opaque user id, so the runtime can admit
//! only a set of user ids. A stream with no §9.12 membership record is DEC-953's one-person
//! workspace: its member is the `user` who recorded the mandate connection's latest
//! `ConnectionEstablished`, the record `mandate approvals approve` reads for DEC-923's check, who
//! holds every workspace role (identity spec §3.2, §4.3). A stream with one, as every workspace
//! `mandate workspace found` founds, is folded by `mandate_identity::Roster` at its latest record's
//! instant, and each reference resolves only among the members active then who hold the approver
//! role in effect. A stream the run cannot read either way resolves no one, so every grant is
//! refused `not_an_approver` and nothing is sent (`AGENTS.md` rule 3).

use std::collections::BTreeSet;

use mandate_canon::{Value, encode_ulid};
use mandate_domain::Environment;
use mandate_identity::{MEMBERSHIP_TYPES, Roster, membership_record};
use mandate_num::Usd;
use mandate_runtime::{LiveBroker, LiveScope, OpaqueUser};
use mandate_spec::Mandate;
use mandate_spec::document::ApproverRef;
use mandate_time::UtcNanos;

use crate::control::ControlRecord;

/// The workspace's members as the run reads them off its control stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Members {
    /// No member the run can name: with no §9.12 record, the connection has no
    /// `ConnectionEstablished`, its latest record is a revocation, or the record names no user;
    /// with one, the stream does not fold (DEC-2384 item 4).
    Unread,
    /// A one-person workspace whose one member, the owner, holds every workspace role.
    Owner(String),
    /// The members the §9.12 fold reads active at the stream's latest record (DEC-2384).
    Folded(Roster),
}

/// The members `records` show for the workspace whose mandate names `connection`. With no §9.12
/// membership record, the owner is the `user` of the connection's latest `ConnectionEstablished`
/// or `ConnectionRevoked` by `seq`, a revocation outranking an establishment at the same `seq`,
/// and only while that record is an establishment naming a user as non-empty text. With one, the
/// stream's [`Roster`] at its latest record, or [`Members::Unread`] when it does not fold.
pub fn members(records: &[ControlRecord], connection: &str) -> Members {
    if records
        .iter()
        .any(|record| MEMBERSHIP_TYPES.contains(&record.event_type.as_str()))
    {
        return roster(records).map_or(Members::Unread, Members::Folded);
    }
    match latest_record(records, connection) {
        Some(record) if record.event_type == "ConnectionEstablished" => text_of(record, "user")
            .filter(|user| !user.is_empty())
            .map_or(Members::Unread, |user| Members::Owner(user.to_owned())),
        Some(_) | None => Members::Unread,
    }
}

/// The roster `records`, in stream order, fold to at the latest record's `event_time`
/// (DEC-2384 items 2 and 4): `None` when any record carries no `event_time`, a membership record
/// does not map, or the fold is unreadable then.
fn roster(records: &[ControlRecord]) -> Option<Roster> {
    let timed: Vec<(&ControlRecord, UtcNanos)> = records
        .iter()
        .map(|record| record.event_time.map(|at| (record, at)))
        .collect::<Option<_>>()?;
    let at = timed.iter().map(|(_, at)| *at).max()?;
    let Ok(memberships) = timed
        .into_iter()
        .filter_map(|(record, at)| {
            membership_record(record.seq, &record.event_type, at, &record.payload).transpose()
        })
        .collect::<Result<Vec<_>, _>>()
    else {
        return None;
    };
    match Roster::read(memberships, at) {
        Ok(roster) => Some(roster),
        Err(_) => None,
    }
}

/// The latest `ConnectionEstablished` or `ConnectionRevoked` of `connection` by `seq`, a
/// revocation outranking an establishment at the same `seq`: the one record both [`members`] and
/// [`live_cli_confirm`] read, so the two never disagree on which record governs.
fn latest_record<'r>(records: &'r [ControlRecord], connection: &str) -> Option<&'r ControlRecord> {
    records
        .iter()
        .filter(|record| {
            matches!(
                record.event_type.as_str(),
                "ConnectionEstablished" | "ConnectionRevoked"
            ) && text_of(record, "connection_id") == Some(connection)
        })
        .max_by_key(|record| (record.seq, record.event_type == "ConnectionRevoked"))
}

/// The user ids `references` resolve to among `members`: the set admission check 3 compares a
/// responder against. Among DEC-953's owner, a role reference resolves to the owner, who holds
/// every workspace role, and a `user:` reference only when it names the owner. Among a roster, see
/// [`folded`]. Empty when nothing resolves, so every grant is refused.
pub fn resolve(references: &[ApproverRef], members: &Members) -> BTreeSet<String> {
    let owner = match members {
        Members::Unread => return BTreeSet::new(),
        Members::Folded(roster) => return folded(references, roster),
        Members::Owner(owner) => owner,
    };
    references
        .iter()
        .filter(|reference| match reference.as_str().split_once(':') {
            Some(("role", _)) => true,
            Some(("user", user)) => user == owner,
            Some(_) | None => false,
        })
        .map(|_| owner.clone())
        .collect()
}

/// The user ids `references` resolve to among `roster`, by [`Roster::approvers`], the one
/// resolution the API's grant check shares (DEC-2384 item 3, DEC-2130), each as its ULID text.
fn folded(references: &[ApproverRef], roster: &Roster) -> BTreeSet<String> {
    let texts: Vec<&str> = references.iter().map(ApproverRef::as_str).collect();
    roster
        .approvers(&texts)
        .into_iter()
        .map(|member| encode_ulid(member.0))
        .collect()
}

/// One text member of `record`'s payload.
fn text_of<'r>(record: &'r ControlRecord, name: &str) -> Option<&'r str> {
    record.payload.get(name).and_then(Value::as_str)
}

/// DEC-923's scope for a live `cli_confirm` grant, read as `mandate approvals approve` reads it
/// (DEC-970 item 1): on a `live` run, the mandate connection's latest record is a `live`
/// `ConnectionEstablished`, its broker names the scope's broker, the confirmed mandate gives the
/// per-order cap and the pinned instruments, and the owner is [`members`]' owner. `None` on a paper
/// run, and whenever any part cannot be read, so the runtime refuses every live `cli_confirm`
/// grant `step_up_method` (`AGENTS.md` rule 3).
pub fn live_cli_confirm(
    records: &[ControlRecord],
    mandate: &Mandate,
    environment: Environment,
) -> Option<LiveScope> {
    if environment != Environment::Live {
        return None;
    }
    let connection = mandate.connection_id.as_str();
    let Members::Owner(owner) = members(records, connection) else {
        return None;
    };
    let latest = latest_record(records, connection)?;
    if text_of(latest, "environment") != Some("live") {
        return None;
    }
    let broker = match text_of(latest, "broker") {
        Some("robinhood") => LiveBroker::Robinhood,
        Some(_) | None => LiveBroker::Other,
    };
    let Ok(max_order_usd) = Usd::parse(mandate.risk.max_order_usd.as_str()) else {
        return None;
    };
    Some(LiveScope {
        broker,
        max_order_usd,
        pinned_instruments: mandate
            .universe
            .pinned_instruments
            .iter()
            .map(|instrument| instrument.asset_id.as_str().to_owned())
            .collect(),
        workspace_owner: OpaqueUser(owner),
    })
}
