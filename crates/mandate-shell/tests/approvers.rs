//! The mandate's approver references resolved before the runtime sees them (DEC-953, DEC-2384;
//! mandate spec §6.4 check 3, V-024). On a stream with no §9.12 record the owner is the `user` who
//! recorded the mandate connection's latest `ConnectionEstablished`, the record `mandate approvals
//! approve` reads for DEC-923's check; on a founded stream the references resolve among the members
//! the membership fold reads active. Nothing here reaches a host or a credential (`AGENTS.md`
//! rule 8). The resolution's own cases moved with it to `mandate-identity`'s `tests/approvers.rs`;
//! these pin what the shell hands the runtime.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use mandate_alpaca::{Exchange, HttpRequest, Response, TradingTransport, TransportError};
use mandate_canon::to_canonical;
use mandate_domain::Environment;
use mandate_shell::adapters::{Sources, production};
use mandate_shell::approvers::{members, resolve};
use mandate_shell::control::{ControlRecord, Origin};
use mandate_shell::paper::load_contexts;
use mandate_shell::symbols::BrokerSymbols;
use mandate_spec::document::ApproverRef;
use mandate_time::{Date, UtcNanos};

use common::{SPY, Stream, json, model, spy_facts, spy_mandate};

/// The connection SPY's mandate names.
const CONNECTION: &str = "conn_alpaca_paper_01";
const FOUNDER: &str = "user_founder";
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

fn references(texts: &[&str]) -> Vec<ApproverRef> {
    texts
        .iter()
        .map(|t| ApproverRef::parse(t).unwrap())
        .collect()
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

/// The founder's principal, an opaque ULID as `mandate workspace found` writes it (identity spec
/// §3.1).
const ULID_FOUNDER: &str = "01K6C00000000000000000000F";

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

/// What the approvers `texts` resolve to over `records`.
fn resolved(records: &[ControlRecord], texts: &[&str]) -> BTreeSet<String> {
    resolve(&references(texts), &members(records, CONNECTION))
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
    assert_eq!(admitted_approvers(&founded), principals(&[ULID_FOUNDER]));
}

/// The run reads a stored envelope's `event_time` only in the journal's canonical form; an
/// envelope with none, or with any other text, carries none, so its stream folds no one.
#[test]
fn a_record_carries_its_envelopes_canonical_event_time_only() {
    let canonical = json(r#"{"event_time":"2026-10-01T00:00:00.000000000Z","payload":{}}"#);
    assert_eq!(ControlRecord::event_time_of(&canonical), Some(at(0)));
    for body in [
        r#"{"payload":{}}"#,
        r#"{"event_time":null}"#,
        r#"{"event_time":7}"#,
        r#"{"event_time":"2026-10-01T00:00:00Z"}"#,
    ] {
        assert_eq!(ControlRecord::event_time_of(&json(body)), None, "{body}");
    }
}

/// E7-31 (DEC-2384 item 3): the CLI's lower-case `--user` names the founder its upper-case
/// founding grant activated, as a responder and as a `user:` reference; a near miss of the
/// founder's id, as either, names no one.
#[test]
fn a_lower_case_cli_responder_is_the_founder_and_no_near_miss_is() {
    let founded = [founding(1, 0, ULID_FOUNDER)];
    let resolved = resolved(&founded, &["role:approver"]);
    assert!(
        resolved.contains(&ULID_FOUNDER.to_ascii_lowercase()),
        "{resolved:?}"
    );
    assert!(resolved.contains(ULID_FOUNDER), "{resolved:?}");
    let lower = format!("user:{}", ULID_FOUNDER.to_ascii_lowercase());
    assert_eq!(resolved_by(&founded, &lower), principals(&[ULID_FOUNDER]));
    let (head, _) = ULID_FOUNDER.split_at(25);
    for miss in [
        format!("user:{head}"),
        format!("user:{ULID_FOUNDER}0"),
        format!("user: {ULID_FOUNDER}"),
        format!("user:{ULID_FOUNDER} "),
        format!("user:{head}E"),
        format!("user:{head}I"),
        format!("user:{head}L"),
        format!("user:{head}O"),
        format!("user:{head}U"),
    ] {
        assert_eq!(resolved_by(&founded, &miss), users(&[]), "{miss:?}");
        assert!(!resolved.contains(&miss[5..]), "{miss:?} as a responder");
    }
}

/// What the one reference `text` resolves to over `records`, or nothing when the mandate schema's
/// own grammar refuses it.
fn resolved_by(records: &[ControlRecord], text: &str) -> BTreeSet<String> {
    ApproverRef::parse(text).map_or_else(
        |_| BTreeSet::new(),
        |reference| resolve(&[reference], &members(records, CONNECTION)),
    )
}
