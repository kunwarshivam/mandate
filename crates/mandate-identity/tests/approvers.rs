//! The mandate's approver references resolved to the user ids admission compares (DEC-953,
//! DEC-2384; mandate spec §6.4 check 3, V-024), over the control stream's records alone. On a
//! stream with no §9.12 record the owner is the `user` who recorded the mandate connection's
//! latest `ConnectionEstablished`; on a founded stream the references resolve among the members
//! the membership fold reads active. Moved with the resolution from the paper shell's tests; a
//! record here is what the shell's `ControlRecord` carries, less its origin, and a reference is
//! the text a mandate's `ApproverRef` holds.

use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_identity::approvers::{self, Members, RecordView};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

/// One control-stream record as the shell reads it, less its origin.
#[derive(Debug, Clone)]
struct ControlRecord {
    seq: u64,
    event_time: Option<UtcNanos>,
    event_type: String,
    payload: Value,
}

/// A mandate's approver reference, as its text.
type ApproverRef = String;

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn views(records: &[ControlRecord]) -> Vec<RecordView<'_>> {
    records
        .iter()
        .map(|record| RecordView {
            seq: record.seq,
            event_type: &record.event_type,
            event_time: record.event_time,
            payload: &record.payload,
        })
        .collect()
}

fn members(records: &[ControlRecord], connection: &str) -> Members {
    approvers::members(&views(records), connection)
}

fn resolve(references: &[ApproverRef], members: &Members) -> BTreeSet<String> {
    let texts: Vec<&str> = references.iter().map(String::as_str).collect();
    approvers::resolve(&texts, members)
}

/// The connection SPY's mandate names.
const CONNECTION: &str = "conn_alpaca_paper_01";
const FOUNDER: &str = "user_founder";
const OTHER: &str = "user_other";

fn record(seq: u64, event_type: &str, payload: &str) -> ControlRecord {
    ControlRecord {
        seq,
        event_time: None,
        event_type: event_type.to_owned(),
        payload: json(payload),
    }
}

/// The CLI's version-2 `ConnectionEstablished` of `connection`, recorded by `user`.
fn established(seq: u64, connection: &str, user: &str) -> ControlRecord {
    record(
        seq,
        "ConnectionEstablished",
        &format!(
            r#"{{"account_ref":"acct_spy","broker":"alpaca","connection_id":"{connection}","environment":"paper","scopes":["trading"],"user":"{user}"}}"#
        ),
    )
}

fn revoked(seq: u64, connection: &str) -> ControlRecord {
    record(
        seq,
        "ConnectionRevoked",
        &format!(r#"{{"connection_id":"{connection}","reason":"owner"}}"#),
    )
}

fn owner(user: &str) -> Members {
    Members::Owner(user.to_owned())
}

fn references(texts: &[&str]) -> Vec<ApproverRef> {
    texts.iter().map(|t| (*t).to_owned()).collect()
}

fn users(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

/// The user ids check 3 compares a responder with for `ids`, ULIDs: each in its canonical upper
/// case and in the lower case the CLI's `--user` takes (DEC-2384 item 3).
fn principals(ids: &[&str]) -> BTreeSet<String> {
    ids.iter()
        .flat_map(|id| [(*id).to_owned(), id.to_ascii_lowercase()])
        .collect()
}

/// The owner is the user on the mandate connection's latest record, read in `seq` order, and only
/// while that record is an establishment naming a user.
#[test]
fn the_owner_is_the_user_who_recorded_the_mandates_connection() {
    assert_eq!(members(&[], CONNECTION), Members::Unread, "no record");
    let ours = [established(1, CONNECTION, FOUNDER)];
    assert_eq!(members(&ours, CONNECTION), owner(FOUNDER), "the record");
    let theirs = [established(1, "conn_other", OTHER)];
    assert_eq!(
        members(&theirs, CONNECTION),
        Members::Unread,
        "another connection"
    );
    let both = [
        established(1, CONNECTION, FOUNDER),
        established(2, "conn_other", OTHER),
    ];
    assert_eq!(members(&both, CONNECTION), owner(FOUNDER), "beside another");
    let gone = [established(1, CONNECTION, FOUNDER), revoked(2, CONNECTION)];
    assert_eq!(members(&gone, CONNECTION), Members::Unread, "revoked");
    let signed = [
        established(1, CONNECTION, FOUNDER),
        record(
            2,
            "ConnectionRevoked",
            &format!(r#"{{"connection_id":"{CONNECTION}","reason":"owner","user":"{FOUNDER}"}}"#),
        ),
    ];
    assert_eq!(
        members(&signed, CONNECTION),
        Members::Unread,
        "a revocation names no owner, whatever it carries"
    );
    let back = [
        established(1, CONNECTION, FOUNDER),
        revoked(2, CONNECTION),
        established(3, CONNECTION, OTHER),
    ];
    assert_eq!(members(&back, CONNECTION), owner(OTHER), "re-established");
    let shuffled = [
        established(3, CONNECTION, OTHER),
        established(1, CONNECTION, FOUNDER),
        revoked(2, CONNECTION),
    ];
    assert_eq!(members(&shuffled, CONNECTION), owner(OTHER), "by seq");
    let unsorted = [revoked(2, CONNECTION), established(1, CONNECTION, FOUNDER)];
    assert_eq!(members(&unsorted, CONNECTION), Members::Unread, "by seq");
    let tied = [revoked(1, CONNECTION), established(1, CONNECTION, FOUNDER)];
    assert_eq!(members(&tied, CONNECTION), Members::Unread, "a tie revokes");
    let tied = [established(1, CONNECTION, FOUNDER), revoked(1, CONNECTION)];
    assert_eq!(members(&tied, CONNECTION), Members::Unread, "either order");
}

/// A record whose user cannot be read names no one.
#[test]
fn an_establishment_with_no_readable_user_names_no_one() {
    for payload in [
        format!(r#"{{"connection_id":"{CONNECTION}","environment":"paper"}}"#),
        format!(r#"{{"connection_id":"{CONNECTION}","environment":"paper","user":null}}"#),
        format!(r#"{{"connection_id":"{CONNECTION}","environment":"paper","user":7}}"#),
        format!(r#"{{"connection_id":"{CONNECTION}","environment":"paper","user":""}}"#),
    ] {
        let records = [record(1, "ConnectionEstablished", &payload)];
        assert_eq!(members(&records, CONNECTION), Members::Unread, "{payload}");
    }
}

/// The founder's and two invitees' principals, opaque ULIDs as `mandate workspace found` and the
/// invitation flow write them (identity spec §3.1), and a user who never joins.
const ULID_FOUNDER: &str = "01K6C00000000000000000000F";
const ULID_B: &str = "01K6C00000000000000000000B";
const ULID_C: &str = "01K6C00000000000000000000C";
const ULID_OUTSIDER: &str = "01K6C00000000000000000000D";

/// The instant `secs` after 2026-10-01T00:00:00Z.
fn at(secs: i64) -> UtcNanos {
    let start = UtcNanos::parse_rfc3339("2026-10-01T00:00:00Z").unwrap();
    UtcNanos::from_parts(start.secs() + secs, 0).unwrap()
}

/// `record` committed at `at(secs)`.
fn timed(seq: u64, secs: i64, event_type: &str, payload: &str) -> ControlRecord {
    ControlRecord {
        event_time: Some(at(secs)),
        ..record(seq, event_type, payload)
    }
}

/// The founding grant `mandate workspace found` writes (DEC-991; journal spec §9.12): the owner
/// bundle, no invitation, no cool-off.
fn founding(seq: u64, secs: i64, member: &str) -> ControlRecord {
    let ends = at(secs);
    timed(
        seq,
        secs,
        "MemberActivated",
        &format!(
            r#"{{"cool_off_ends_at":"{ends}","independent_approval_required":false,"invitation":null,"member":"{member}","reason":"founding","roles":["approver","operator","workspace_admin"]}}"#
        ),
    )
}

/// `roles`' codes as rule 96 writes them: ascending by their bytes.
fn codes(roles: &[&str]) -> String {
    let mut sorted = roles.to_vec();
    sorted.sort_unstable();
    let quoted: Vec<String> = sorted.iter().map(|r| format!(r#""{r}""#)).collect();
    quoted.join(",")
}

fn invited(seq: u64, secs: i64, invitation: &str, roles: &[&str]) -> ControlRecord {
    let expires = at(secs + 7 * 86_400);
    let roles = codes(roles);
    timed(
        seq,
        secs,
        "MemberInvited",
        &format!(r#"{{"expires_at":"{expires}","invitation":"{invitation}","roles":[{roles}]}}"#),
    )
}

/// The activation accepting `invitation`, cooling off for `cool` seconds.
fn accepted(
    seq: u64,
    secs: i64,
    member: &str,
    invitation: &str,
    roles: &[&str],
    cool: i64,
) -> ControlRecord {
    let (ends, roles) = (at(secs + cool), codes(roles));
    timed(
        seq,
        secs,
        "MemberActivated",
        &format!(
            r#"{{"cool_off_ends_at":"{ends}","independent_approval_required":false,"invitation":"{invitation}","member":"{member}","reason":"invitation_accepted","roles":[{roles}]}}"#
        ),
    )
}

fn deactivated(seq: u64, secs: i64, member: &str) -> ControlRecord {
    timed(
        seq,
        secs,
        "MemberDeactivated",
        &format!(r#"{{"member":"{member}"}}"#),
    )
}

/// The connection's establishment by `user`, committed at `at(secs)`.
fn established_at(seq: u64, secs: i64, user: &str) -> ControlRecord {
    ControlRecord {
        event_time: Some(at(secs)),
        ..established(seq, CONNECTION, user)
    }
}

/// What the approvers `texts` resolve to over `records`.
fn resolved(records: &[ControlRecord], texts: &[&str]) -> BTreeSet<String> {
    resolve(&references(texts), &members(records, CONNECTION))
}

/// E7-31 (DEC-2384): in a workspace `mandate workspace found` founded, as the local cell founds
/// it, each role reference and the founder's own `user:` reference resolve to the founder, whoever
/// recorded the connection; another user's reference resolves to no one.
#[test]
fn a_founded_workspace_resolves_each_reference_to_the_founder() {
    let founded = [
        founding(1, 0, ULID_FOUNDER),
        established_at(2, 10, "user_founder"),
    ];
    for role in ["role:approver", "role:owner", "role:workspace_admin"] {
        assert_eq!(
            resolved(&founded, &[role]),
            principals(&[ULID_FOUNDER]),
            "{role}"
        );
    }
    let mine = format!("user:{ULID_FOUNDER}");
    assert_eq!(resolved(&founded, &[&mine]), principals(&[ULID_FOUNDER]));
    let theirs = format!("user:{ULID_OUTSIDER}");
    assert_eq!(resolved(&founded, &[&theirs]), users(&[]), "not a member");
    assert_eq!(
        resolved(&founded, &["user:user_founder"]),
        users(&[]),
        "the connection's user"
    );
    let alone = [founding(1, 0, ULID_FOUNDER)];
    assert_eq!(
        resolved(&alone, &["role:approver"]),
        principals(&[ULID_FOUNDER]),
        "no connection"
    );
}

/// E7-31: an invitee is an approver only once its cool-off has ended by the stream's latest
/// membership record (DEC-2384 item 2), only while it holds the approver role, and never once deactivated; `role:owner` names
/// the founder alone, and `role:workspace_admin` only an approver who is also workspace admin.
#[test]
fn an_invitee_resolves_only_while_an_active_approver() {
    let inv_b = "01K6C0000000000000000001B0";
    let joined = vec![
        founding(1, 0, ULID_FOUNDER),
        invited(2, 10, inv_b, &["approver", "workspace_admin"]),
        accepted(
            3,
            20,
            ULID_B,
            inv_b,
            &["approver", "workspace_admin"],
            1_000,
        ),
    ];
    let later = |seq, secs| invited(seq, secs, "01K6C0000000000000000009V0", &["viewer"]);
    let beside = [joined.clone(), vec![established_at(4, 1_020, "u")]].concat();
    let both = principals(&[ULID_FOUNDER]);
    assert_eq!(
        resolved(&beside, &["role:approver"]),
        both,
        "a later record of another type does not end the cool-off"
    );
    let cooling = [joined.clone(), vec![later(4, 500)]].concat();
    let both = principals(&[ULID_FOUNDER]);
    assert_eq!(resolved(&cooling, &["role:approver"]), both, "B cools off");
    let cooled = [joined.clone(), vec![later(4, 1_020)]].concat();
    let both = principals(&[ULID_FOUNDER, ULID_B]);
    assert_eq!(resolved(&cooled, &["role:approver"]), both, "B cooled off");
    assert_eq!(
        resolved(&cooled, &["role:workspace_admin"]),
        both,
        "B is admin"
    );
    let owner = principals(&[ULID_FOUNDER]);
    assert_eq!(
        resolved(&cooled, &["role:owner"]),
        owner,
        "the founder alone"
    );
    let b = format!("user:{ULID_B}");
    assert_eq!(resolved(&cooled, &[&b]), principals(&[ULID_B]));
    let gone = [cooled.clone(), vec![deactivated(5, 1_030, ULID_B)]].concat();
    assert_eq!(resolved(&gone, &[&b]), users(&[]), "deactivated");
    let founder_gone = [cooled, vec![deactivated(5, 1_030, ULID_FOUNDER)]].concat();
    assert_eq!(
        resolved(&founder_gone, &["role:owner"]),
        users(&[]),
        "the founder left"
    );
    let plain = vec![
        founding(1, 0, ULID_FOUNDER),
        invited(2, 10, inv_b, &["approver"]),
        accepted(3, 20, ULID_B, inv_b, &["approver"], 0),
    ];
    let approvers = principals(&[ULID_FOUNDER, ULID_B]);
    assert_eq!(
        resolved(&plain, &["role:approver"]),
        approvers,
        "B approves"
    );
    let admins = resolved(&plain, &["role:workspace_admin"]);
    assert_eq!(
        admins,
        principals(&[ULID_FOUNDER]),
        "an approver who is not workspace admin"
    );
    let viewer = vec![
        founding(1, 0, ULID_FOUNDER),
        invited(2, 10, inv_b, &["viewer", "workspace_admin"]),
        accepted(3, 20, ULID_B, inv_b, &["viewer", "workspace_admin"], 0),
    ];
    assert_eq!(resolved(&viewer, &[&b]), users(&[]), "no approver role");
    let admins = resolved(&viewer, &["role:workspace_admin"]);
    assert_eq!(
        admins,
        principals(&[ULID_FOUNDER]),
        "an admin who cannot approve"
    );
}

/// A membership stream the run cannot fold resolves no one: a record with no `event_time`, a
/// founding grant that does not map, a deactivation of no member, a member founded twice, and a
/// record of another type with no `event_time` (DEC-2384 item 3; `AGENTS.md` rule 3).
#[test]
fn an_unreadable_membership_stream_resolves_no_one() {
    let untimed = ControlRecord {
        event_time: None,
        ..founding(1, 0, ULID_FOUNDER)
    };
    let unmapped = timed(
        1,
        0,
        "MemberActivated",
        &format!(r#"{{"invitation":null,"member":"{ULID_FOUNDER}","reason":"founding"}}"#),
    );
    let cases = [
        ("no event_time", vec![untimed]),
        ("unmapped", vec![unmapped]),
        (
            "a deactivation of no member",
            vec![founding(1, 0, ULID_FOUNDER), deactivated(2, 10, ULID_B)],
        ),
        (
            "founded twice",
            vec![founding(1, 0, ULID_FOUNDER), founding(2, 10, ULID_FOUNDER)],
        ),
        (
            "an untimed record beside",
            vec![
                founding(1, 0, ULID_FOUNDER),
                established(2, CONNECTION, FOUNDER),
            ],
        ),
        (
            "out of order",
            vec![
                founding(2, 10, ULID_FOUNDER),
                deactivated(1, 0, ULID_FOUNDER),
            ],
        ),
    ];
    let every = ["role:approver", "role:owner", "role:workspace_admin"];
    for (case, records) in cases {
        assert_eq!(members(&records, CONNECTION), Members::Unread, "{case}");
        assert_eq!(resolved(&records, &every), users(&[]), "{case}");
        let mine = format!("user:{ULID_FOUNDER}");
        assert_eq!(resolved(&records, &[&mine]), users(&[]), "{case}");
    }
}

const ULIDS: [&str; 4] = [ULID_FOUNDER, ULID_B, ULID_C, ULID_OUTSIDER];
const GRANTABLE: [&str; 4] = ["approver", "operator", "viewer", "workspace_admin"];

/// One invitee as the history draws it: whether it joins, its role mask over [`GRANTABLE`], a
/// long cool-off or none, and whether it is deactivated.
type Invitee = (bool, u8, bool, bool);

/// One drawn history: the invitees, whether the founder is deactivated, whether the stream's
/// latest record falls after every long cool-off, the role references, the users named, and the
/// responder.
type Drawn = (
    Vec<Invitee>,
    bool,
    bool,
    Vec<&'static str>,
    Vec<&'static str>,
    usize,
);

fn invitee() -> impl Strategy<Value = Invitee> {
    (any::<bool>(), 1..16u8, any::<bool>(), any::<bool>())
}

/// E7-31: over random founded workspaces, check 3's membership test on the resolved set admits
/// a responder exactly when it is an active approver at the stream's latest record and a
/// reference names it: `role:approver`, `role:workspace_admin` while it is also workspace
/// admin, `role:owner` while it is the founder, or `user:` with its id. The oracle reads the
/// drawn history and the reference texts itself, never the fold or `resolve`.
#[test]
fn a_responder_is_an_approver_exactly_when_an_active_approver_answers_a_reference_naming_them() {
    let strategy = (
        proptest::collection::vec(invitee(), 2),
        any::<bool>(),
        any::<bool>(),
        proptest::sample::subsequence(ROLES.to_vec(), 0..=3),
        proptest::sample::subsequence(ULIDS.to_vec(), 0..=4),
        0..ULIDS.len(),
    );
    let config = ProptestConfig {
        cases: 512,
        failure_persistence: None,
        ..Default::default()
    };
    let body = |(invitees, founder_leaves, late, roles, named, responder): Drawn| -> Result<(), TestCaseError> {
        let mut records = vec![founding(1, 0, ULID_FOUNDER)];
        let mut seq = 1;
        let mut eligible: BTreeSet<&str> = BTreeSet::new();
        let mut admin: BTreeSet<&str> = BTreeSet::from([ULID_FOUNDER]);
        let mut leaving = Vec::new();
        let tail = if late { 10_000 } else { 500 };
        for (k, (joins, mask, long, leaves)) in invitees.iter().enumerate() {
            let member = ULIDS[k + 1];
            let invitation = format!("01K6C000000000000000000{}V0", k + 1);
            let granted: Vec<&str> = (0..4).filter(|i| mask >> i & 1 == 1).map(|i| GRANTABLE[i]).collect();
            let base = 10 + 20 * i64::try_from(k).unwrap();
            seq += 1;
            records.push(invited(seq, base, &invitation, &granted));
            if !joins {
                continue;
            }
            let cool = if *long { 5_000 } else { 0 };
            seq += 1;
            records.push(accepted(seq, base + 5, member, &invitation, &granted, cool));
            let cooled = base + 5 + cool <= tail;
            if *leaves {
                leaving.push(member);
            } else {
                if cooled && granted.contains(&"approver") {
                    eligible.insert(member);
                }
                if granted.contains(&"workspace_admin") {
                    admin.insert(member);
                }
            }
        }
        for (k, member) in leaving.into_iter().enumerate() {
            seq += 1;
            records.push(deactivated(seq, 100 + i64::try_from(k).unwrap(), member));
        }
        if founder_leaves {
            seq += 1;
            records.push(deactivated(seq, 150, ULID_FOUNDER));
        } else {
            eligible.insert(ULID_FOUNDER);
        }
        seq += 1;
        records.push(invited(seq, tail, "01K6C0000000000000000009V0", &["viewer"]));
        seq += 1;
        records.push(established_at(seq, tail + 50_000, "user_founder"));
        let texts: Vec<String> = roles
            .iter()
            .map(|r| (*r).to_owned())
            .chain(named.iter().map(|u| format!("user:{u}")))
            .collect();
        let refs: Vec<ApproverRef> = texts.iter().cloned().collect();
        let responder = ULIDS[responder];
        let resolved = resolve(&refs, &members(&records, CONNECTION));
        let names = texts.iter().any(|t| match t.as_str() {
            "role:approver" => true,
            "role:workspace_admin" => admin.contains(responder),
            "role:owner" => responder == ULID_FOUNDER,
            user => user == format!("user:{responder}"),
        });
        let expected = eligible.contains(responder) && names;
        prop_assert_eq!(resolved.contains(responder), expected, "{:?} {:?}", texts, eligible);
        prop_assert!(resolved
            .iter()
            .all(|u| eligible.contains(u.to_ascii_uppercase().as_str())), "{:?}", resolved);
        Ok(())
    };
    if let Err(failure) = TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

/// Each role reference and the owner's own `user:` reference resolve to the owner; another user's
/// reference and every reference among unread members resolve to no one.
#[test]
fn each_reference_resolves_to_the_owner_and_nothing_else() {
    let founder = owner(FOUNDER);
    for role in ["role:approver", "role:owner", "role:workspace_admin"] {
        assert_eq!(
            resolve(&references(&[role]), &founder),
            users(&[FOUNDER]),
            "{role}"
        );
        assert_eq!(
            resolve(&references(&[role]), &Members::Unread),
            users(&[]),
            "{role}"
        );
    }
    let mine = references(&["user:user_founder"]);
    assert_eq!(resolve(&mine, &founder), users(&[FOUNDER]));
    let theirs = references(&["user:user_other"]);
    assert_eq!(resolve(&theirs, &founder), users(&[]), "not a member");
    assert_eq!(resolve(&mine, &Members::Unread), users(&[]));
    let every = references(&["role:approver", "role:owner", "user:user_founder"]);
    assert_eq!(resolve(&every, &founder), users(&[FOUNDER]), "once");
    let mixed = references(&["user:user_other", "role:approver"]);
    assert_eq!(resolve(&mixed, &founder), users(&[FOUNDER]));
    assert_eq!(resolve(&[], &founder), users(&[]), "no reference");
}

const POOL: [&str; 3] = [FOUNDER, OTHER, "user_third"];
const ROLES: [&str; 3] = ["role:approver", "role:owner", "role:workspace_admin"];

proptest! {
    /// Over any reference set, any members and any responder, check 3's membership test on the
    /// resolved set admits exactly the owner, and only when a reference names them. The oracle
    /// reads the reference texts itself, not `resolve`.
    #[test]
    fn a_responder_is_an_approver_exactly_when_the_owner_answers_a_reference_naming_them(
        roles in proptest::sample::subsequence(ROLES.to_vec(), 0..=3),
        named in proptest::sample::subsequence(POOL.to_vec(), 0..=3),
        member in proptest::option::of(0..POOL.len()),
        responder in 0..POOL.len(),
    ) {
        let texts: Vec<String> = roles
            .iter()
            .map(|r| (*r).to_owned())
            .chain(named.iter().map(|u| format!("user:{u}")))
            .collect();
        let refs: Vec<ApproverRef> =
            texts.iter().cloned().collect();
        let members = member.map_or(Members::Unread, |i| owner(POOL[i]));
        let responder = POOL[responder];
        let resolved = resolve(&refs, &members);
        let expected = member.is_some_and(|i| {
            POOL[i] == responder
                && texts
                    .iter()
                    .any(|t| t.starts_with("role:") || *t == format!("user:{responder}"))
        });
        prop_assert_eq!(resolved.contains(responder), expected);
        prop_assert!(resolved.len() <= 1, "one owner at most: {:?}", resolved);
        if let Some(i) = member {
            prop_assert!(resolved.iter().all(|u| u == POOL[i]));
        } else {
            prop_assert!(resolved.is_empty());
        }
    }
}

/// DEC-2384 item 6: a grant is judged against the roster as of its own record. Through the
/// invitee's activation it resolves; through its later deactivation it does not; a record after
/// `seq` never counts.
#[test]
fn members_through_a_seq_read_only_the_records_up_to_it() {
    let inv_b = "01K6C0000000000000000001B0";
    let records = vec![
        founding(1, 0, ULID_FOUNDER),
        invited(2, 10, inv_b, &["approver"]),
        accepted(3, 20, ULID_B, inv_b, &["approver"], 0),
        deactivated(4, 40, ULID_B),
    ];
    let b = format!("user:{ULID_B}");
    let through = |seq| {
        resolve(
            &references(&[&b]),
            &approvers::members_through(&views(&records), CONNECTION, seq),
        )
    };
    assert_eq!(
        through(3),
        principals(&[ULID_B]),
        "active at its activation"
    );
    assert_eq!(through(4), users(&[]), "deactivated by seq 4");
    assert_eq!(through(9), users(&[]), "past the end");
    assert_eq!(
        approvers::members_through(&views(&records), CONNECTION, 0),
        Members::Unread,
        "before any record"
    );
    let owned = [established(1, CONNECTION, FOUNDER), revoked(2, CONNECTION)];
    assert_eq!(
        approvers::members_through(&views(&owned), CONNECTION, 1),
        owner(FOUNDER),
        "before its revocation"
    );
}
