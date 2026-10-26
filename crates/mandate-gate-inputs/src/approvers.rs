//! The mandate's approver references resolved to the user ids admission compares (mandate spec
//! §6.4 check 3, V-024; [DEC-953](../../../docs/project/decisions/DEC-953.md)).
//!
//! A mandate names its approvers as `role:approver`, `role:owner`, `role:workspace_admin` or
//! `user:<id>`, while an answer names its responder by opaque user id, so the runtime can admit
//! only a set of user ids. The run reads the workspace's members from the one identity record the
//! founder's control stream journals, the same one `mandate approvals approve` reads for DEC-923's
//! check: the `user` who recorded the mandate connection's latest `ConnectionEstablished`, the
//! workspace owner. In that one-person workspace the owner holds every workspace role (identity
//! spec §3.2, §4.3), so each role reference and the owner's own `user:` reference resolve to the
//! owner, and nothing else resolves. A stream the run cannot read that way resolves no one, so
//! every grant is refused `not_an_approver` and nothing is sent (`AGENTS.md` rule 3).

use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_runtime::{LiveBroker, LiveScope, OpaqueUser};
use mandate_spec::Mandate;
use mandate_spec::document::ApproverRef;

use crate::control::ControlRecord;

/// The workspace's members as the run reads them off its control stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Members {
    /// No member the run can name: the connection has no `ConnectionEstablished`, its latest
    /// record is a revocation, the record names no user, or the stream holds a §9.12 membership
    /// record the run does not fold, so the workspace may have more than one member.
    Unread,
    /// A one-person workspace whose one member, the owner, holds every workspace role.
    Owner(String),
}

/// The §9.12 membership records: any of them means a membership the run does not fold.
const MEMBERSHIP_RECORDS: [&str; 7] = [
    "MemberInvited",
    "MemberInvitationRevoked",
    "MemberActivated",
    "MemberRoleChanged",
    "MemberDeactivated",
    "MemberReactivated",
    "MemberRemoved",
];

/// The members `records` show for the workspace whose mandate names `connection`: the owner is
/// the `user` of the connection's latest `ConnectionEstablished` or `ConnectionRevoked` by `seq`,
/// a revocation outranking an establishment at the same `seq`, and only while that record is an
/// establishment naming a user as non-empty text. Any §9.12 membership record leaves them
/// [`Members::Unread`].
pub fn members(records: &[ControlRecord], connection: &str) -> Members {
    if records
        .iter()
        .any(|record| MEMBERSHIP_RECORDS.contains(&record.event_type.as_str()))
    {
        return Members::Unread;
    }
    match latest_record(records, connection) {
        Some(record) if record.event_type == "ConnectionEstablished" => text_of(record, "user")
            .filter(|user| !user.is_empty())
            .map_or(Members::Unread, |user| Members::Owner(user.to_owned())),
        Some(_) | None => Members::Unread,
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
/// responder against. A role reference resolves to the owner, who holds every workspace role; a
/// `user:` reference resolves only when it names the owner, since no one else is a member. Empty
/// when nothing resolves, so every grant is refused.
pub fn resolve(references: &[ApproverRef], members: &Members) -> BTreeSet<String> {
    let Members::Owner(owner) = members else {
        return BTreeSet::new();
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
