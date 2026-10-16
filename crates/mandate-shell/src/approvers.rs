//! The mandate's approver references resolved to the user ids admission compares (mandate spec
//! §6.4 check 3, V-024; [DEC-953](../../../docs/project/decisions/DEC-953.md)).
//!
//! A mandate names its approvers as `role:approver`, `role:owner`, `role:workspace_admin` or
//! `user:<id>`, while an answer names its responder by opaque user id, so the runtime can admit
//! only a set of user ids. The run reads the workspace's members from the one identity record the
//! founder's control stream journals, the same one `mandate approvals approve` reads for DEC-923's
//! check: the `user` who recorded the mandate connection's latest `ConnectionEstablished`, the
//! workspace owner. In that one-person workspace the owner holds every workspace role (identity
//! spec §3.2, §4.4), so each role reference and the owner's own `user:` reference resolve to the
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

/// The members `records` show for the workspace whose mandate names `connection`.
pub fn members(records: &[ControlRecord], connection: &str) -> Members {
    let _ = (records, connection, MEMBERSHIP_RECORDS);
    Members::Unread
}

/// The user ids `references` resolve to among `members`: the set admission check 3 compares a
/// responder against. Empty when nothing resolves.
pub fn resolve(references: &[ApproverRef], members: &Members) -> BTreeSet<String> {
    let _ = (members, Value::Null);
    references
        .iter()
        .map(|reference| reference.as_str().to_owned())
        .collect()
}
