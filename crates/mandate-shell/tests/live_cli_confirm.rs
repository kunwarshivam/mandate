//! DEC-923's live `cli_confirm` scope, read by the run as `mandate approvals approve` reads it,
//! and handed to the runtime's admission (R0's R16, DEC-970 items 1 to 3). On a live run whose
//! mandate connection's latest record is a live `ConnectionEstablished` recorded by the workspace
//! owner, the scope names that record's broker, the confirmed mandate's per-order cap and pinned
//! instrument, and the owner. Every other stream, and every paper run, gives no scope, so the
//! runtime still refuses a live `cli_confirm` grant `step_up_method`. Nothing here reaches a host
//! or a credential (`AGENTS.md` rule 8).

mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use mandate_alpaca::{HttpRequest, Response, TradingTransport, TransportError};
use mandate_canon::to_canonical;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_runtime::{LiveBroker, LiveScope, OpaqueUser};
use mandate_shell::adapters::{Sources, production};
use mandate_shell::approvers::live_cli_confirm;
use mandate_shell::control::{ControlRecord, Origin};
use mandate_shell::paper::load_contexts;
use mandate_shell::symbols::BrokerSymbols;
use mandate_spec::Mandate;
use mandate_time::{Date, UtcNanos};

use common::{SPY, Stream, json, model, spy_facts, spy_mandate};

/// The connection SPY's mandate names.
const CONNECTION: &str = "conn_alpaca_paper_01";
const FOUNDER: &str = "user_founder";
/// The per-order cap this suite writes into the mandate: DEC-529 item 2's.
const CAP: &str = "100";
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
            environment: Environment::Live,
        },
    }
}

/// The CLI's version-2 `ConnectionEstablished` of `connection` on `broker` in `environment`,
/// recorded by `user`.
fn established(
    seq: u64,
    connection: &str,
    broker: &str,
    environment: &str,
    user: &str,
) -> ControlRecord {
    record(
        seq,
        "ConnectionEstablished",
        &format!(
            r#"{{"account_ref":"acct_spy","broker":"{broker}","connection_id":"{connection}","environment":"{environment}","scopes":["get_accounts"],"user":"{user}"}}"#
        ),
    )
}

fn live_robinhood(seq: u64) -> ControlRecord {
    established(seq, CONNECTION, "robinhood", "live", FOUNDER)
}

fn revoked(seq: u64) -> ControlRecord {
    record(
        seq,
        "ConnectionRevoked",
        &format!(r#"{{"connection_id":"{CONNECTION}","reason":"owner"}}"#),
    )
}

/// SPY's mandate with its per-order cap set to [`CAP`].
fn mandate() -> Mandate {
    let text = spy_mandate(&model());
    let key = text.find(r#""max_order_usd""#).unwrap() + r#""max_order_usd""#.len();
    let start = key + text[key..].find('"').unwrap() + 1;
    let end = start + text[start..].find('"').unwrap();
    let text = format!("{}{CAP}{}", &text[..start], &text[end..]);
    Mandate::parse(&json(&text)).unwrap()
}

/// The scope the suite expects, built from its own constants.
fn expected(broker: LiveBroker) -> LiveScope {
    LiveScope {
        broker,
        max_order_usd: Usd::parse(CAP).unwrap(),
        pinned_instruments: [SPY.to_owned()].into(),
        workspace_owner: OpaqueUser(FOUNDER.to_owned()),
    }
}

#[test]
fn a_live_robinhood_record_by_the_owner_gives_the_scope() {
    let scope = live_cli_confirm(&[live_robinhood(1)], &mandate(), Environment::Live);
    assert_eq!(scope, Some(expected(LiveBroker::Robinhood)));
}

#[test]
fn a_paper_run_gives_no_scope_whatever_the_record() {
    assert_eq!(
        live_cli_confirm(&[live_robinhood(1)], &mandate(), Environment::Paper),
        None
    );
}

#[test]
fn a_record_that_is_not_live_gives_no_scope() {
    let paper = established(1, CONNECTION, "robinhood", "paper", FOUNDER);
    assert_eq!(
        live_cli_confirm(&[paper], &mandate(), Environment::Live),
        None
    );
}

#[test]
fn another_broker_is_scoped_as_other_and_admission_refuses_it() {
    let alpaca = established(1, CONNECTION, "alpaca", "live", FOUNDER);
    assert_eq!(
        live_cli_confirm(&[alpaca], &mandate(), Environment::Live),
        Some(expected(LiveBroker::Other))
    );
}

#[test]
fn the_latest_record_of_the_mandates_connection_governs() {
    let mandate = mandate();
    let scoped = |records: &[ControlRecord]| live_cli_confirm(records, &mandate, Environment::Live);
    assert_eq!(scoped(&[live_robinhood(1), revoked(2)]), None, "revoked");
    assert_eq!(
        scoped(&[live_robinhood(1), revoked(2), live_robinhood(3)]),
        Some(expected(LiveBroker::Robinhood)),
        "re-established"
    );
    assert_eq!(
        scoped(&[live_robinhood(2), revoked(2)]),
        None,
        "a revocation outranks an establishment at the same seq"
    );
    assert_eq!(scoped(&[]), None, "no record");
    let unrelated = record(
        2,
        "MandateConfirmed",
        &format!(r#"{{"connection_id":"{CONNECTION}","environment":"paper"}}"#),
    );
    assert_eq!(
        scoped(&[live_robinhood(1), unrelated]),
        Some(expected(LiveBroker::Robinhood)),
        "a later record of another type naming the connection is not its record"
    );
    let elsewhere = established(2, "conn_other", "robinhood", "paper", FOUNDER);
    assert_eq!(
        scoped(&[live_robinhood(1), elsewhere]),
        Some(expected(LiveBroker::Robinhood)),
        "a later record of another connection is not its record"
    );
    let other = established(1, "conn_other", "robinhood", "live", FOUNDER);
    assert_eq!(scoped(&[other]), None, "another connection's record");
}

#[test]
fn a_record_naming_no_owner_gives_no_scope() {
    let mandate = mandate();
    let scoped = |records: &[ControlRecord]| live_cli_confirm(records, &mandate, Environment::Live);
    let empty = established(1, CONNECTION, "robinhood", "live", "");
    assert_eq!(scoped(&[empty]), None, "an empty user");
    let unnamed = record(
        1,
        "ConnectionEstablished",
        &format!(
            r#"{{"account_ref":"acct_spy","broker":"robinhood","connection_id":"{CONNECTION}","environment":"live","scopes":["get_accounts"]}}"#
        ),
    );
    assert_eq!(scoped(&[unnamed]), None, "no user");
    let member = record(2, "MemberInvited", r#"{"user":"user_other"}"#);
    assert_eq!(
        scoped(&[live_robinhood(1), member]),
        None,
        "a membership stream gives no live scope (DEC-2384 item 5)"
    );
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

/// The scope the production mandate stage hands the runtime when the run context carries `scope`.
fn admitted_scope(scope: Option<LiveScope>) -> Option<LiveScope> {
    let now = UtcNanos::parse_rfc3339(TUESDAY).unwrap();
    let mut stream = Stream::deployed(
        &common::SNAPSHOT.replace("2026-09-21", "2026-09-25"),
        common::RULES,
        common::FEE,
    );
    stream.record(
        "ConnectionEstablished",
        &format!(
            r#"{{"account_ref":"acct_spy","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","scopes":["trading"],"user":"{FOUNDER}"}}"#
        ),
    );
    let artifacts = stream
        .artifacts_on(Date::parse("2026-09-29").unwrap())
        .unwrap();
    let deployed = artifacts
        .deployment("ws_spy".into(), "agent_spy".into(), "acct_spy".into())
        .unwrap();
    let agent = deployed.deployment().agent.clone();
    let facts = spy_facts(SPY, "SPY", mandate_alpaca::Exchange::Arca, now);
    let mut contexts = load_contexts(&artifacts, &facts, now, &agent, Environment::Paper).unwrap();
    contexts.run.live_cli_confirm = scope;
    let file = MANDATE_FILE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!(
            "mandate-shell-live-cli-confirm-{}.json",
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
    stages
        .mandate
        .admitted()
        .unwrap()
        .view
        .approval
        .live_cli_confirm
}

/// The production view carries the run context's scope to the runtime's admission, and no scope
/// when the context has none, so the grant is refused exactly as before.
#[test]
fn the_production_view_hands_the_runtime_the_run_contexts_scope() {
    let scope = expected(LiveBroker::Robinhood);
    assert_eq!(admitted_scope(Some(scope.clone())), Some(scope));
    assert_eq!(admitted_scope(None), None);
}
