//! E9-7 M1a (DEC-657): the membership fold against journal spec §9.12's `membership_fold` vectors,
//! which `reference/journal/membership_fold.py` reproduces and whose expectations are written by
//! hand from identity spec §5 and §8.3. Each history's records are numbered from 1, as the vectors'
//! `refused` entries number them; records that are not membership records are dropped, as the
//! store's adapter drops them.

use std::collections::BTreeSet;
use std::path::Path;

use mandate_canon::{Value, decode_ulid, parse};
use mandate_identity::{
    InvitationId, MembershipEvent, MembershipFold, MembershipRecord, PrincipalId, Role,
};
use mandate_time::UtcNanos;

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn ulid(value: &Value, name: &str) -> u128 {
    decode_ulid(text(value, name)).unwrap()
}

fn role(name: &str) -> Role {
    match name {
        "approver" => Role::Approver,
        "auditor" => Role::Auditor,
        "operator" => Role::Operator,
        "viewer" => Role::Viewer,
        "workspace_admin" => Role::WorkspaceAdmin,
        other => panic!("`{other}` is not a §9.12 role"),
    }
}

fn roles(value: &Value, name: &str) -> BTreeSet<Role> {
    list(value, name)
        .iter()
        .map(|r| role(r.as_str().unwrap()))
        .collect()
}

fn independent(payload: &Value) -> bool {
    payload.get("independent_approval_required") == Some(&Value::Bool(true))
}

/// The record a vector's control-stream record maps to, or `None` for one that is not a
/// membership record.
fn record(seq: u64, raw: &Value) -> Option<MembershipRecord> {
    let p = raw.get("payload").unwrap();
    let member = || PrincipalId(ulid(p, "member"));
    let event = match text(raw, "event_type") {
        "MemberInvited" => MembershipEvent::Invited {
            invitation: InvitationId(ulid(p, "invitation")),
            roles: roles(p, "roles"),
            expires_at: instant(text(p, "expires_at")),
        },
        "MemberInvitationRevoked" => MembershipEvent::InvitationRevoked {
            invitation: InvitationId(ulid(p, "invitation")),
        },
        "MemberActivated" => MembershipEvent::Activated {
            member: member(),
            invitation: p
                .get("invitation")
                .and_then(Value::as_str)
                .map(|t| InvitationId(decode_ulid(t).unwrap())),
            roles: roles(p, "roles"),
            independent_approval_required: independent(p),
            cool_off_ends_at: instant(text(p, "cool_off_ends_at")),
        },
        "MemberRoleChanged" => MembershipEvent::RoleChanged {
            member: member(),
            added: list(p, "added")
                .iter()
                .map(|a| (role(text(a, "role")), instant(text(a, "cool_off_ends_at"))))
                .collect(),
            removed: roles(p, "removed"),
            independent_approval_required: independent(p),
        },
        "MemberDeactivated" => MembershipEvent::Deactivated { member: member() },
        "MemberReactivated" => MembershipEvent::Reactivated {
            member: member(),
            roles: roles(p, "roles"),
            independent_approval_required: independent(p),
            cool_off_ends_at: instant(text(p, "cool_off_ends_at")),
        },
        "MemberRemoved" => MembershipEvent::Removed { member: member() },
        _ => return None,
    };
    Some(MembershipRecord::new(
        seq,
        instant(text(raw, "event_time")),
        event,
    ))
}

/// A state's name as the vectors spell it, from its `Debug` name: `CoolingOff` is `cooling_off`.
fn spelled(state: impl std::fmt::Debug) -> String {
    format!("{state:?}")
        .chars()
        .enumerate()
        .flat_map(|(i, c)| {
            [
                (i > 0 && c.is_uppercase()).then_some('_'),
                Some(c.to_ascii_lowercase()),
            ]
        })
        .flatten()
        .collect()
}

/// §9.12's fold, §5.1's states, §5.3 and ID-7's count, and §8.3's per-role cool-off: every probe of
/// every history, its unreadable latch, and the record it refuses.
#[test]
#[ignore = "pending E9-7"]
fn the_fold_reproduces_every_membership_fold_vector() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let histories = list(fixture.get("membership_fold").unwrap(), "histories");
    assert!(!histories.is_empty(), "the fixture holds no histories");
    for history in histories {
        let name = text(history, "name");
        let raw = list(history, "records");
        let fold = MembershipFold::new(
            raw.iter()
                .zip(1..)
                .filter_map(|(r, seq)| record(seq, r))
                .collect(),
        );
        let end = instant(text(raw.last().unwrap(), "event_time"));
        let unreadable = history.get("unreadable") == Some(&Value::Bool(true));
        assert_eq!(fold.unreadable(end), unreadable, "{name}: unreadable");
        match list(history, "refused").first() {
            Some(refused) => assert_eq!(
                fold.refused(end),
                refused.get("record").and_then(Value::as_int),
                "{name}: the refused record"
            ),
            None => assert_eq!(fold.refused(end).is_some(), unreadable, "{name}: refused"),
        }
        for probe in list(history, "probes") {
            let at = instant(text(probe, "at"));
            let users = probe.get("workspace_users").and_then(Value::as_int);
            assert_eq!(
                Some(u64::from(fold.workspace_users(at))),
                users,
                "{name} at {at:?}: workspace_users"
            );
            for m in list(probe, "members") {
                let member = PrincipalId(ulid(m, "member"));
                let got = fold.state(member, at).map(spelled);
                assert_eq!(got.as_deref(), Some(text(m, "state")), "{name} at {at:?}");
            }
            for m in list(probe, "roles") {
                let member = PrincipalId(ulid(m, "member"));
                let want = roles(m, "roles");
                assert_eq!(fold.effective_roles(member, at), want, "{name} at {at:?}");
            }
            for i in list(probe, "invitations") {
                let invitation = InvitationId(ulid(i, "invitation"));
                let got = fold.invitation(invitation, at).map(spelled);
                assert_eq!(got.as_deref(), Some(text(i, "state")), "{name} at {at:?}");
            }
        }
    }
}
