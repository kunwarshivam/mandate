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
    let text = |record: &ControlRecord, name: &str| {
        record
            .payload
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let latest = records
        .iter()
        .filter(|record| {
            matches!(
                record.event_type.as_str(),
                "ConnectionEstablished" | "ConnectionRevoked"
            ) && text(record, "connection_id").as_deref() == Some(connection)
        })
        .max_by_key(|record| (record.seq, record.event_type == "ConnectionRevoked"));
    match latest {
        Some(record) if record.event_type == "ConnectionEstablished" => text(record, "user")
            .filter(|user| !user.is_empty())
            .map_or(Members::Unread, Members::Owner),
        Some(_) | None => Members::Unread,
    }
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
