//! A mandate's approver references resolved to the user ids admission compares (mandate spec
//! §6.4 check 3, V-024; DEC-953, DEC-2384), over the control stream's records alone, so every
//! process that admits a grant, the paper shell, the executor's gate inputs and the runtime daemon,
//! resolves it the same way.
//!
//! A mandate names its approvers as `role:approver`, `role:owner`, `role:workspace_admin` or
//! `user:<id>`, while an answer names its responder by opaque user id, so the runtime can admit
//! only a set of user ids. A stream with no §9.12 membership record is DEC-953's one-person
//! workspace: its member is the `user` who recorded the mandate connection's latest
//! `ConnectionEstablished`, who holds every workspace role (identity spec §3.2, §4.3). A stream
//! with one is folded by [`Roster`] at its latest membership record's instant, and each reference
//! resolves only among the members active then who hold the approver role in effect. A stream that
//! cannot be read either way resolves no one, so every grant is refused (`AGENTS.md` rule 3).

use std::collections::BTreeSet;

use mandate_canon::{Value, encode_ulid};
use mandate_time::UtcNanos;

use crate::record::{MEMBERSHIP_TYPES, membership_record};
use crate::roster::Roster;

/// What the resolution reads of one control-stream record: its sequence number, type, envelope
/// `event_time` (`None` when the reader has none) and payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordView<'r> {
    pub seq: u64,
    pub event_type: &'r str,
    pub event_time: Option<UtcNanos>,
    pub payload: &'r Value,
}

/// The workspace's members as a run reads them off its control stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Members {
    /// No member the run can name: with no §9.12 record, the connection has no
    /// `ConnectionEstablished`, its latest record is a revocation, or the record names no user;
    /// with one, the stream does not fold (DEC-2384 item 4).
    Unread,
    /// A one-person workspace whose one member, the owner, holds every workspace role.
    Owner(String),
    /// The members the §9.12 fold reads active at the stream's latest membership record (DEC-2384).
    Folded(Roster),
}

/// The members `records` show for the workspace whose mandate names `connection`. With no §9.12
/// membership record, the owner is the `user` of the connection's latest `ConnectionEstablished`
/// or `ConnectionRevoked` by `seq`, a revocation outranking an establishment at the same `seq`,
/// and only while that record is an establishment naming a user as non-empty text. With one, the
/// stream's [`Roster`] at its latest membership record, or [`Members::Unread`] when it does not fold.
pub fn members(records: &[RecordView<'_>], connection: &str) -> Members {
    if records
        .iter()
        .any(|record| MEMBERSHIP_TYPES.contains(&record.event_type))
    {
        return roster(records).map_or(Members::Unread, Members::Folded);
    }
    match latest_connection_record(records, connection) {
        Some(record) if record.event_type == "ConnectionEstablished" => text_of(record, "user")
            .filter(|user| !user.is_empty())
            .map_or(Members::Unread, |user| Members::Owner(user.to_owned())),
        Some(_) | None => Members::Unread,
    }
}

/// [`members`] of `records` up to and including `seq`: a grant is judged against the roster as of
/// its own record, so a membership record later in the same read neither lends it a role its
/// member lacked when it was made nor takes one away (DEC-2384 item 6). `records` must already be
/// in stream order, as [`members`] requires: the filter keeps their order, and a roster read out
/// of order does not fold, so it resolves no one.
pub fn members_through(records: &[RecordView<'_>], connection: &str, seq: u64) -> Members {
    let judged: Vec<RecordView<'_>> = records
        .iter()
        .filter(|record| record.seq <= seq)
        .copied()
        .collect();
    members(&judged, connection)
}

/// The roster `records`, in stream order, fold to at the latest membership record's
/// `event_time` (DEC-2384 items 2 and 4): `None` when any record carries no `event_time`, a
/// membership record does not map, or the fold is unreadable then.
fn roster(records: &[RecordView<'_>]) -> Option<Roster> {
    let timed: Vec<(&RecordView<'_>, UtcNanos)> = records
        .iter()
        .map(|record| record.event_time.map(|at| (record, at)))
        .collect::<Option<_>>()?;
    let at = timed
        .iter()
        .filter(|(record, _)| MEMBERSHIP_TYPES.contains(&record.event_type))
        .map(|(_, at)| *at)
        .max()?;
    let Ok(memberships) = timed
        .into_iter()
        .filter_map(|(record, at)| {
            membership_record(record.seq, record.event_type, at, record.payload).transpose()
        })
        .collect::<Result<Vec<_>, _>>()
    else {
        return None;
    };
    let Ok(roster) = Roster::read(memberships, at) else {
        return None;
    };
    Some(roster)
}

/// The latest `ConnectionEstablished` or `ConnectionRevoked` of `connection` by `seq`, a
/// revocation outranking an establishment at the same `seq`: the one record [`members`] and the
/// live `cli_confirm` scope both read, so the two never disagree on which record governs.
pub fn latest_connection_record<'v, 'r>(
    records: &'v [RecordView<'r>],
    connection: &str,
) -> Option<&'v RecordView<'r>> {
    records
        .iter()
        .filter(|record| {
            matches!(
                record.event_type,
                "ConnectionEstablished" | "ConnectionRevoked"
            ) && text_of(record, "connection_id") == Some(connection)
        })
        .max_by_key(|record| (record.seq, record.event_type == "ConnectionRevoked"))
}

/// The user ids `references` resolve to among `members`: the set admission check 3 compares a
/// responder against. Among DEC-953's owner, a role reference resolves to the owner, who holds
/// every workspace role, and a `user:` reference only when it names the owner. Among a roster,
/// [`Roster::approvers`] in both ULID spellings. Empty when nothing resolves, so every grant is
/// refused.
pub fn resolve(references: &[&str], members: &Members) -> BTreeSet<String> {
    let owner = match members {
        Members::Unread => return BTreeSet::new(),
        Members::Folded(roster) => return folded(references, roster),
        Members::Owner(owner) => owner,
    };
    references
        .iter()
        .filter(|reference| match reference.split_once(':') {
            Some(("role", _)) => true,
            Some(("user", user)) => user == owner,
            Some(_) | None => false,
        })
        .map(|_| owner.clone())
        .collect()
}

/// The user ids `references` resolve to among `roster`, by [`Roster::approvers`], the one
/// resolution the API's grant check shares (DEC-2384 item 3, DEC-2130): each member's ULID in both
/// of its spellings, the canonical upper case the API's session names and the lower case the
/// CLI's `--user` takes (DEC-527 item 3), so check 3's comparison of a responder's text admits the
/// same principal either way and no one else.
fn folded(references: &[&str], roster: &Roster) -> BTreeSet<String> {
    roster
        .approvers(references)
        .into_iter()
        .flat_map(|member| {
            let canonical = encode_ulid(member.0);
            [canonical.to_ascii_lowercase(), canonical]
        })
        .collect()
}

/// One text member of `record`'s payload.
fn text_of<'r>(record: &RecordView<'r>, name: &str) -> Option<&'r str> {
    record.payload.get(name).and_then(Value::as_str)
}
