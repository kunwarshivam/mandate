//! A stored control-stream record as the membership fold and the writer read it (journal spec
//! §9.12, DEC-991 item 4): each of the seven `Member*` types maps to its [`MembershipRecord`], and
//! every other type is a [`ControlEntry::Other`] with its `seq` and `event_time`. The workspace
//! store's adapters and the founding bootstrap share this one mapping, so the seal stays inside
//! this crate and no adapter writes its own reading of a payload.
//!
//! A membership record whose members do not map is never skipped: it is an [`UnreadableRecord`],
//! and the caller commits nothing on it and grants nothing from it (identity spec ID-7, `AGENTS.md`
//! rule 3). The mapping reads only the members the fold and the writer read; the journal's own
//! rules 96 to 106 are `append`'s, and a record this crate reads was admitted by them.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, decode_ulid};
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;

use crate::writer::ControlEntry;
use crate::{InvitationId, MembershipEvent, MembershipRecord, PrincipalId, Role};

/// The seven `Member*` event types of journal spec §9.12, the only records of a membership.
pub const MEMBERSHIP_TYPES: [&str; 7] = [
    "MemberInvited",
    "MemberInvitationRevoked",
    "MemberActivated",
    "MemberRoleChanged",
    "MemberDeactivated",
    "MemberReactivated",
    "MemberRemoved",
];

/// A stored record that does not map: its `seq` and the envelope or payload member at fault.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the control-stream record at seq {seq} does not map at {member}")]
pub struct UnreadableRecord {
    /// The record's `seq`.
    pub seq: u64,
    /// The member at fault, dotted from the envelope.
    pub member: &'static str,
}

/// The record at `seq` whose envelope is `envelope`, as a [`ControlEntry`].
///
/// # Errors
/// [`UnreadableRecord`] for an envelope with no `event_time` or `event_type`, or a `Member*`
/// record whose payload does not map.
pub fn control_entry(seq: u64, envelope: &Value) -> Result<ControlEntry, UnreadableRecord> {
    let fault = |member| UnreadableRecord { seq, member };
    let event_type = envelope
        .get("event_type")
        .and_then(Value::as_str)
        .ok_or(fault("event_type"))?;
    let event_time = envelope
        .get("event_time")
        .and_then(Value::as_str)
        .and_then(|t| UtcNanos::parse(t).ok())
        .ok_or(fault("event_time"))?;
    if !MEMBERSHIP_TYPES.contains(&event_type) {
        return Ok(ControlEntry::Other { seq, event_time });
    }
    let payload = envelope.get("payload").ok_or(fault("payload"))?;
    mapped(seq, event_type, event_time, payload).map(ControlEntry::Membership)
}

/// The membership record at `seq` of type `event_type`, committed at `event_time`, whose envelope's
/// `payload` is `payload`, mapped as [`control_entry`] maps it; `None` for any other type. For a
/// reader that holds a record's members rather than its envelope (DEC-2384).
///
/// # Errors
/// [`UnreadableRecord`] for a `Member*` record whose payload does not map.
pub fn membership_record(
    seq: u64,
    event_type: &str,
    event_time: UtcNanos,
    payload: &Value,
) -> Result<Option<MembershipRecord>, UnreadableRecord> {
    if !MEMBERSHIP_TYPES.contains(&event_type) {
        return Ok(None);
    }
    mapped(seq, event_type, event_time, payload).map(Some)
}

/// The `Member*` record `event_type` at `seq`, its payload mapped.
fn mapped(
    seq: u64,
    event_type: &str,
    event_time: UtcNanos,
    payload: &Value,
) -> Result<MembershipRecord, UnreadableRecord> {
    let fault = |member| UnreadableRecord { seq, member };
    let payload = Payload {
        value: payload,
        seq,
    };
    let event = match event_type {
        "MemberInvited" => MembershipEvent::Invited {
            invitation: payload.invitation("payload.invitation")?,
            roles: payload.roles("roles", "payload.roles")?,
            expires_at: payload.instant("expires_at", "payload.expires_at")?,
        },
        "MemberInvitationRevoked" => MembershipEvent::InvitationRevoked {
            invitation: payload.invitation("payload.invitation")?,
        },
        "MemberActivated" => {
            let founding = match payload.text("reason") {
                Some("founding") => true,
                Some("invitation_accepted") => false,
                _ => return Err(fault("payload.reason")),
            };
            let invitation = match payload.value.get("invitation") {
                Some(Value::Null) if founding => None,
                Some(_) if !founding => Some(payload.invitation("payload.invitation")?),
                _ => return Err(fault("payload.invitation")),
            };
            MembershipEvent::Activated {
                member: payload.member()?,
                invitation,
                roles: payload.roles("roles", "payload.roles")?,
                independent_approval_required: payload.independence()?,
                cool_off_ends_at: payload
                    .instant("cool_off_ends_at", "payload.cool_off_ends_at")?,
            }
        }
        "MemberRoleChanged" => MembershipEvent::RoleChanged {
            member: payload.member()?,
            added: payload.added()?,
            removed: payload.roles("removed", "payload.removed")?,
            independent_approval_required: payload.independence()?,
        },
        "MemberDeactivated" => MembershipEvent::Deactivated {
            member: payload.member()?,
        },
        "MemberReactivated" => MembershipEvent::Reactivated {
            member: payload.member()?,
            roles: payload.roles("roles", "payload.roles")?,
            independent_approval_required: payload.independence()?,
            cool_off_ends_at: payload.instant("cool_off_ends_at", "payload.cool_off_ends_at")?,
        },
        _ => MembershipEvent::Removed {
            member: payload.member()?,
        },
    };
    Ok(MembershipRecord::new(Seal::grant(), seq, event_time, event))
}

/// A membership record's payload, read member by member.
struct Payload<'a> {
    value: &'a Value,
    seq: u64,
}

impl Payload<'_> {
    fn fault(&self, member: &'static str) -> UnreadableRecord {
        UnreadableRecord {
            seq: self.seq,
            member,
        }
    }

    fn text(&self, member: &str) -> Option<&str> {
        self.value.get(member).and_then(Value::as_str)
    }

    fn ulid(&self, member: &str, path: &'static str) -> Result<u128, UnreadableRecord> {
        self.text(member)
            .and_then(|t| decode_ulid(t).ok())
            .ok_or_else(|| self.fault(path))
    }

    fn member(&self) -> Result<PrincipalId, UnreadableRecord> {
        self.ulid("member", "payload.member").map(PrincipalId)
    }

    fn invitation(&self, path: &'static str) -> Result<InvitationId, UnreadableRecord> {
        self.ulid("invitation", path).map(InvitationId)
    }

    fn instant(&self, member: &str, path: &'static str) -> Result<UtcNanos, UnreadableRecord> {
        self.text(member)
            .and_then(|t| UtcNanos::parse(t).ok())
            .ok_or_else(|| self.fault(path))
    }

    fn independence(&self) -> Result<bool, UnreadableRecord> {
        match self.value.get("independent_approval_required") {
            Some(Value::Bool(required)) => Ok(*required),
            _ => Err(self.fault("payload.independent_approval_required")),
        }
    }

    /// A role list, strictly ascending by its codes' bytes as rule 96 writes it, each a workspace
    /// role; anything else does not map.
    fn roles(&self, member: &str, path: &'static str) -> Result<BTreeSet<Role>, UnreadableRecord> {
        let codes: Vec<&str> = self
            .value
            .get(member)
            .and_then(Value::as_array)
            .ok_or_else(|| self.fault(path))?
            .iter()
            .map(|code| code.as_str().ok_or_else(|| self.fault(path)))
            .collect::<Result<_, _>>()?;
        if !codes.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
            return Err(self.fault(path));
        }
        codes
            .into_iter()
            .map(|code| Role::from_code(code).ok_or_else(|| self.fault(path)))
            .collect()
    }

    /// `MemberRoleChanged.added`: each granted role, ascending by code with none repeated, and the
    /// end of its cool-off.
    fn added(&self) -> Result<BTreeMap<Role, UtcNanos>, UnreadableRecord> {
        let path = "payload.added";
        let entries = self
            .value
            .get("added")
            .and_then(Value::as_array)
            .ok_or_else(|| self.fault(path))?;
        let mut codes = Vec::new();
        let mut added = BTreeMap::new();
        for entry in entries {
            let code = entry
                .get("role")
                .and_then(Value::as_str)
                .ok_or_else(|| self.fault(path))?;
            let role = Role::from_code(code).ok_or_else(|| self.fault(path))?;
            let ends = entry
                .get("cool_off_ends_at")
                .and_then(Value::as_str)
                .and_then(|t| UtcNanos::parse(t).ok())
                .ok_or_else(|| self.fault(path))?;
            codes.push(code);
            added.insert(role, ends);
        }
        if !codes.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
            return Err(self.fault(path));
        }
        Ok(added)
    }
}
