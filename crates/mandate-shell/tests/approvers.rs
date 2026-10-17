//! The mandate's approver references resolved to the workspace owner before the runtime sees them
//! (DEC-953; mandate spec §6.4 check 3, V-024). The owner is the `user` who recorded the mandate
//! connection's latest `ConnectionEstablished`, the record `mandate approvals approve` reads for
//! DEC-923's check. Nothing here reaches a host or a credential (`AGENTS.md` rule 8).

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

/// Any §9.12 membership record means a membership the run does not fold, before or after the
/// connection's record, so no one is named.
#[test]
fn a_membership_record_leaves_the_members_unread() {
    for event_type in [
        "MemberInvited",
        "MemberInvitationRevoked",
        "MemberActivated",
        "MemberRoleChanged",
        "MemberDeactivated",
        "MemberReactivated",
        "MemberRemoved",
    ] {
        let after = [
            established(1, CONNECTION, FOUNDER),
            record(2, event_type, r#"{"member":"01K6C000000000000000000000"}"#),
        ];
        assert_eq!(members(&after, CONNECTION), Members::Unread, "{event_type}");
        let before = [
            record(1, event_type, r#"{"member":"01K6C000000000000000000000"}"#),
            established(2, CONNECTION, FOUNDER),
        ];
        assert_eq!(
            members(&before, CONNECTION),
            Members::Unread,
            "{event_type}"
        );
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
