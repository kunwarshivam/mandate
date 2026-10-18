//! The bodies of a deployment's calls (DEC-984 item 2): canonical JSON (journal spec §4), decoded
//! here, in the core, so what the plane acts on is exactly what the deployment signed.

use std::collections::BTreeSet;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::Role;

use crate::directory::{Ident, Member, MemberState};
use crate::fleet::{Component, Health, Heartbeat};

/// The most components one heartbeat names.
pub const MAX_COMPONENTS: usize = 64;

/// The most memberships one sync carries.
pub const MAX_MEMBERS: usize = 10_000;

/// Each role's spelling, in `Role::ALL`'s order (identity spec §4.1).
const ROLES: [&str; 8] = [
    "org_owner",
    "org_admin",
    "billing_admin",
    "workspace_admin",
    "operator",
    "approver",
    "viewer",
    "auditor",
];

/// A role's spelling on the wire.
pub fn role_name(role: Role) -> &'static str {
    Role::ALL
        .iter()
        .zip(ROLES)
        .find(|(r, _)| **r == role)
        .map_or("viewer", |(_, name)| name)
}

fn role(text: &str) -> Option<Role> {
    Role::ALL
        .iter()
        .zip(ROLES)
        .find(|(_, name)| *name == text)
        .map(|(r, _)| *r)
}

/// The body as a closed object of exactly `members`, in canonical form; `None` otherwise.
fn object(body: &[u8], members: &[&str]) -> Option<Value> {
    let value = parse(body).ok()?;
    let keys: Vec<&str> = value.as_object()?.keys().map(|k| k.as_str()).collect();
    let mut want = members.to_vec();
    want.sort_unstable();
    (to_canonical(&value) == body && keys == want).then_some(value)
}

fn text<'a>(value: &'a Value, member: &str) -> Option<&'a str> {
    value.get(member).and_then(Value::as_str)
}

fn ident(value: &Value, member: &str) -> Option<Ident> {
    text(value, member).and_then(Ident::parse)
}

/// `{"release_digest": hex, "components": [{"health", "name", "version"}]}`, names unique.
pub(crate) fn heartbeat(body: &[u8]) -> Option<Heartbeat> {
    let value = object(body, &["components", "release_digest"])?;
    let release_digest = text(&value, "release_digest").and_then(Digest::from_hex)?;
    let items = value.get("components").and_then(Value::as_array)?;
    if items.len() > MAX_COMPONENTS {
        return None;
    }
    let components = items
        .iter()
        .map(|c| {
            let keys: Vec<&str> = c.as_object()?.keys().map(|k| k.as_str()).collect();
            if keys != ["health", "name", "version"] {
                return None;
            }
            let health = match text(c, "health")? {
                "ok" => Health::Ok,
                "degraded" => Health::Degraded,
                "down" => Health::Down,
                _ => return None,
            };
            Component::new(ident(c, "name")?, text(c, "version")?, health)
        })
        .collect::<Option<Vec<_>>>()?;
    let names: BTreeSet<&Ident> = components.iter().map(|c| &c.name).collect();
    (names.len() == components.len()).then_some(Heartbeat {
        release_digest,
        components,
    })
}

/// `{"members": [{"role", "state", "user_id", "workspace_id"}]}`, each user once per workspace.
pub(crate) fn members(body: &[u8]) -> Option<Vec<Member>> {
    let value = object(body, &["members"])?;
    let items = value.get("members").and_then(Value::as_array)?;
    if items.len() > MAX_MEMBERS {
        return None;
    }
    let members = items
        .iter()
        .map(|m| {
            let keys: Vec<&str> = m.as_object()?.keys().map(|k| k.as_str()).collect();
            if keys != ["role", "state", "user_id", "workspace_id"] {
                return None;
            }
            let state = match text(m, "state")? {
                "active" => MemberState::Active,
                "deactivated" => MemberState::Deactivated,
                _ => return None,
            };
            Some(Member {
                user_id: ident(m, "user_id")?,
                workspace_id: ident(m, "workspace_id")?,
                role: role(text(m, "role")?)?,
                state,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let pairs: BTreeSet<(&Ident, &Ident)> = members
        .iter()
        .map(|m| (&m.user_id, &m.workspace_id))
        .collect();
    (pairs.len() == members.len()).then_some(members)
}

/// `{"user_id": id}`.
pub(crate) fn user(body: &[u8]) -> Option<Ident> {
    ident(&object(body, &["user_id"])?, "user_id")
}
