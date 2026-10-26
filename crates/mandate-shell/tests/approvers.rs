//! The mandate's approver references resolved before the runtime sees them (DEC-953, DEC-2384;
//! mandate spec §6.4 check 3, V-024). On a stream with no §9.12 record the owner is the `user` who
//! recorded the mandate connection's latest `ConnectionEstablished`, the record `mandate approvals
//! approve` reads for DEC-923's check; on a founded stream the references resolve among the members
//! the membership fold reads active. Nothing here reaches a host or a credential (`AGENTS.md`
//! rule 8).

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use mandate_alpaca::{Exchange, HttpRequest, Response, TradingTransport, TransportError};
use mandate_canon::to_canonical;
use mandate_domain::Environment;
use mandate_shell::adapters::{Sources, production};
use mandate_shell::approvers::{Members, members, resolve};
use mandate_shell::control::{ControlRecord, Origin};
use mandate_shell::paper::load_contexts;
use mandate_shell::symbols::BrokerSymbols;
use mandate_spec::document::ApproverRef;
use mandate_time::{Date, UtcNanos};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use common::{SPY, Stream, json, model, spy_facts, spy_mandate};

/// The connection SPY's mandate names.
const CONNECTION: &str = "conn_alpaca_paper_01";
const FOUNDER: &str = "user_founder";
const OTHER: &str = "user_other";
/// Tuesday 13:00 in New York.
const TUESDAY: &str = "2026-09-29T17:00:00Z";

fn record(seq: u64, event_type: &str, payload: &str) -> ControlRecord {
    ControlRecord {
        seq,
        event_time: None,
        event_type: event_type.to_owned(),
        payload: json(payload),
        origin: Origin {
            stream_id: "ctl:ws_spy".to_owned(),
            environment: Environment::Paper,
        },
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
    texts
        .iter()
        .map(|t| ApproverRef::parse(t).unwrap())
        .collect()
}

fn users(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
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
#[ignore = "pending E7-31"]
fn a_founded_workspace_resolves_each_reference_to_the_founder() {
    let founded = [
        founding(1, 0, ULID_FOUNDER),
        established_at(2, 10, "user_founder"),
    ];
    for role in ["role:approver", "role:owner", "role:workspace_admin"] {
        assert_eq!(
            resolved(&founded, &[role]),
            users(&[ULID_FOUNDER]),
            "{role}"
        );
    }
    let mine = format!("user:{ULID_FOUNDER}");
    assert_eq!(resolved(&founded, &[&mine]), users(&[ULID_FOUNDER]));
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
        users(&[ULID_FOUNDER]),
        "no connection"
    );
}

/// E7-31: an invitee is an approver only once its cool-off has ended by the stream's latest
/// record, only while it holds the approver role, and never once deactivated; `role:owner` names
/// the founder alone, and `role:workspace_admin` only an approver who is also workspace admin.
#[test]
#[ignore = "pending E7-31"]
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
    let cooling = [joined.clone(), vec![established_at(4, 500, "u")]].concat();
    let both = users(&[ULID_FOUNDER]);
    assert_eq!(resolved(&cooling, &["role:approver"]), both, "B cools off");
    let cooled = [joined.clone(), vec![established_at(4, 1_020, "u")]].concat();
    let both = users(&[ULID_FOUNDER, ULID_B]);
    assert_eq!(resolved(&cooled, &["role:approver"]), both, "B cooled off");
    assert_eq!(
        resolved(&cooled, &["role:workspace_admin"]),
        both,
        "B is admin"
    );
    let owner = users(&[ULID_FOUNDER]);
    assert_eq!(
        resolved(&cooled, &["role:owner"]),
        owner,
        "the founder alone"
    );
    let b = format!("user:{ULID_B}");
    assert_eq!(resolved(&cooled, &[&b]), users(&[ULID_B]));
    let gone = [cooled.clone(), vec![deactivated(5, 1_030, ULID_B)]].concat();
    assert_eq!(resolved(&gone, &[&b]), users(&[]), "deactivated");
    let founder_gone = [cooled, vec![deactivated(5, 1_030, ULID_FOUNDER)]].concat();
    assert_eq!(
        resolved(&founder_gone, &["role:owner"]),
        users(&[]),
        "the founder left"
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
        users(&[ULID_FOUNDER]),
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
#[ignore = "pending E7-31"]
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
        records.push(established_at(seq, tail, "user_founder"));
        let texts: Vec<String> = roles
            .iter()
            .map(|r| (*r).to_owned())
            .chain(named.iter().map(|u| format!("user:{u}")))
            .collect();
        let refs: Vec<ApproverRef> = texts.iter().map(|t| ApproverRef::parse(t).unwrap()).collect();
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
        prop_assert!(resolved.iter().all(|u| eligible.contains(u.as_str())), "{:?}", resolved);
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
            texts.iter().map(|t| ApproverRef::parse(t).unwrap()).collect();
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

/// A broker the mandate stage never reaches.
#[derive(Clone, Default)]
struct Unreached;

impl TradingTransport for Unreached {
    async fn send(&self, _: &HttpRequest) -> Result<Response, TransportError> {
        panic!("admitting the mandate reads no broker")
    }
}

static MANDATE_FILE: OnceLock<PathBuf> = OnceLock::new();

/// The approvers the production mandate stage hands the runtime for SPY's deployment on `stream`.
fn admitted_approvers(stream: &Stream) -> BTreeSet<String> {
    let now = UtcNanos::parse_rfc3339(TUESDAY).unwrap();
    let artifacts = stream
        .artifacts_on(Date::parse("2026-09-29").unwrap())
        .unwrap();
    let deployed = artifacts
        .deployment("ws_spy".into(), "agent_spy".into(), "acct_spy".into())
        .unwrap();
    let agent = deployed.deployment().agent.clone();
    let facts = spy_facts(SPY, "SPY", Exchange::Arca, now);
    let contexts = load_contexts(
        &artifacts,
        &facts,
        now,
        &agent,
        mandate_domain::Environment::Paper,
    )
    .unwrap();
    let file = MANDATE_FILE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!(
            "mandate-shell-approvers-{}.json",
            std::process::id()
        ));
        fs::write(&path, to_canonical(&json(&spy_mandate(&model())))).unwrap();
        path
    });
    let stages = production(Sources {
        mandate: file.clone(),
        journal: None,
        recorded_at: now,
        agent,
        workspace: "ws_spy".to_owned(),
        account_ref: "acct_spy".to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: None,
        content_store: None,
        symbols: BrokerSymbols::default(),
        transport: Unreached,
    });
    stages.mandate.admitted().unwrap().view.approval.approvers
}

/// The instrument snapshot, classified recently enough for a Tuesday run.
fn snapshot() -> String {
    common::SNAPSHOT.replace("2026-09-21", "2026-09-25")
}

/// The production view carries the owner the stream names, never the mandate's `role:approver`;
/// with no connection record it carries no one, so every grant is refused.
#[test]
fn the_production_view_hands_the_runtime_the_owner_not_the_role() {
    let mut recorded = Stream::deployed(&snapshot(), common::RULES, common::FEE);
    let payload = format!(
        r#"{{"account_ref":"acct_spy","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","scopes":["trading"],"user":"{FOUNDER}"}}"#
    );
    recorded.record("ConnectionEstablished", &payload);
    assert_eq!(admitted_approvers(&recorded), users(&[FOUNDER]));
    let unrecorded = Stream::deployed(&snapshot(), common::RULES, common::FEE);
    assert_eq!(admitted_approvers(&unrecorded), users(&[]));
}

/// E7-31 (DEC-2384): on a stream `mandate workspace found` founded, as the local cell's is, the
/// production view hands the runtime the founder, so check 3 admits the founder's grant, the
/// web app's and the CLI's alike, and no one else's.
#[test]
#[ignore = "pending E7-31"]
fn the_production_view_hands_the_runtime_the_founder_of_a_founded_workspace() {
    let mut founded = Stream::deployed(&snapshot(), common::RULES, common::FEE);
    let ends = founded.records.len() + 1;
    let start = UtcNanos::parse_rfc3339("2026-09-20T00:00:00Z").unwrap();
    let ends = UtcNanos::from_parts(start.secs() + i64::try_from(ends).unwrap(), 0).unwrap();
    founded.record(
        "MemberActivated",
        &format!(
            r#"{{"cool_off_ends_at":"{ends}","independent_approval_required":false,"invitation":null,"member":"{ULID_FOUNDER}","reason":"founding","roles":["approver","operator","workspace_admin"]}}"#
        ),
    );
    let payload = format!(
        r#"{{"account_ref":"acct_spy","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","scopes":["trading"],"user":"{FOUNDER}"}}"#
    );
    founded.record("ConnectionEstablished", &payload);
    assert_eq!(admitted_approvers(&founded), users(&[ULID_FOUNDER]));
}
