//! DEC-923 in `mandate approvals approve` (DEC-529 item 3, DEC-927): on a `live` agent stream the
//! CLI commits a grant only when the mandate the request binds, read from the store, names a
//! recorded, unrevoked Robinhood `live` connection, caps `max_order_usd` at 100 USD and pins one
//! instrument, the bound order is within both, and the owner running it recorded that connection.
//! Every refusal commits nothing; the one admission commits one `ApprovalResponseSubmitted` naming
//! the owner and `cli_confirm`. A skip is never held (PX-7), and paper is unchanged.

mod common;

use std::num::NonZeroU8;

use common::{AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OWNER, TIMEOUT_S, at, body, code_of};
use mandate_approval::{
    AskablePurpose, AssetClass, BoundAction, ReferenceMark, RequestContent, RiskClock,
};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::approvals::{approve, skip};
use mandate_cli::control::{ControlError, Owner, Submitted};
use mandate_journal::Environment;
use mandate_num::{Price, Qty, Signed};

const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const CONNECTION: &str = "conn_rh_live_01";

/// The fixture mandate made DEC-529's: `live`, on the Robinhood connection, capped at 100 USD,
/// with each `(from, to)` edit applied after.
fn live_mandate(edits: &[(&str, &str)]) -> Vec<u8> {
    let mut text = MANDATE
        .replace(r#""environment":"paper""#, r#""environment":"live""#)
        .replace("conn_alpaca_paper_01", CONNECTION)
        .replace(r#""max_order_usd":"300""#, r#""max_order_usd":"100""#);
    for (from, to) in edits {
        assert!(text.contains(from), "{from}");
        text = text.replace(from, to);
    }
    to_canonical(&parse(text.as_bytes()).unwrap())
}

/// Stores `document` and returns its version reference.
fn store(fx: &mut Fixture, document: Vec<u8>) -> String {
    let digest = Digest::of(&document);
    fx.store.insert(digest, document);
    format!("sha256:{}", digest.to_hex())
}

/// The control services' record of the connection: `broker`, `environment`, recorded by `user`.
fn connection(fx: &mut Fixture, broker: &str, environment: &str, user: &str) {
    connection_of(fx, CONNECTION, broker, environment, user);
}

/// The control services' record of connection `id`.
fn connection_of(fx: &mut Fixture, id: &str, broker: &str, environment: &str, user: &str) {
    let payload = common::object(&[
        ("account_ref", common::text("01J0ACC0VNT000000000000000")),
        ("broker", common::text(broker)),
        ("connection_id", common::text(id)),
        ("environment", common::text(environment)),
        ("margin_attestation", common::text("cash_account")),
        ("scopes", Value::Array(vec![common::text("trade")])),
        ("user", common::text(user)),
    ]);
    fx.append_in(environment, CONTROL, "ConnectionEstablished", None, payload);
}

fn revoked(fx: &mut Fixture) {
    let payload = common::object(&[("connection_id", common::text(CONNECTION))]);
    fx.append_in("live", CONTROL, "ConnectionRevoked", None, payload);
}

/// `qty` shares of `instrument` at 89.5 USD under `version`.
fn request(instrument: &str, qty: &str, version: &str) -> RequestContent {
    RequestContent {
        bound: BoundAction {
            instrument: instrument.to_owned(),
            asset_class: AssetClass::UsEquity,
            qty: Qty::parse(qty).unwrap(),
            limit: Price::parse("89.5").unwrap(),
            purpose: AskablePurpose::Open,
            mandate_version: version.to_owned(),
            decided_by: "rule:big-order".to_owned(),
            combined_score: Signed::parse("0.5").unwrap(),
            reference_mark: Some(ReferenceMark {
                price: Price::parse("89.5").unwrap(),
                seq: 2,
            }),
            approvers_required: NonZeroU8::MIN,
            independent_required: false,
        },
        evidence: Vec::new(),
        risk_impact: Vec::new(),
        deadline: RiskClock(ASKED_AT + TIMEOUT_S),
        execution: None,
        deferred_to_binding: Vec::new(),
    }
}

/// DEC-529's deployment, recorded and stored, with one live request for one share of SPY.
fn dec_529(fx: &mut Fixture) -> common::Asked {
    let version = store(fx, live_mandate(&[]));
    connection(fx, "robinhood", "live", OWNER);
    fx.ask_in("live", AGENT, &request(SPY, "1", &version))
}

fn responses(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL)
        .iter()
        .map(body)
        .filter(|e| common::member_text(e, "event_type") == Some("ApprovalResponseSubmitted"))
        .collect()
}

fn granting(fx: &mut Fixture, asked: &common::Asked) -> Result<Submitted, ControlError> {
    approve(
        &mut fx.journal,
        &fx.store,
        &mut FixedIds::default(),
        &live_owner(),
        AGENT,
        &asked.approval,
        &code_of(&asked.content),
        at(ASKED_AT + 5),
    )
}

/// The owner, acting in the live workspace its control stream is (DEC-952).
fn live_owner() -> Owner {
    Owner {
        environment: Environment::Live,
        ..common::owner()
    }
}

fn refused_with(fx: &mut Fixture, asked: &common::Asked, reason: &str) {
    let result = granting(fx, asked);
    assert!(
        matches!(&result, Err(ControlError::Refused { reason: r }) if *r == reason),
        "{reason}: {result:?}"
    );
    assert!(
        responses(fx).is_empty(),
        "{reason}: a refusal commits nothing"
    );
}

/// DEC-923 items 1 and 2: the one admission commits the grant with the owner and `cli_confirm`.
#[test]
fn the_owners_grant_on_dec_529s_deployment_is_committed() {
    let mut fx = Fixture::new();
    let asked = dec_529(&mut fx);
    let submitted = granting(&mut fx, &asked).unwrap_or_else(|e| panic!("{e}"));
    let committed = responses(&fx);
    assert_eq!(committed.len(), 1);
    let grant = &committed[0];
    assert_eq!(
        common::member_text(grant, "event_id"),
        Some(submitted.event_id.as_str())
    );
    assert_eq!(
        common::member_text(grant, "payload.verdict"),
        Some("approved")
    );
    assert_eq!(common::member_text(grant, "payload.responder"), Some(OWNER));
    assert_eq!(
        common::member_text(grant, "payload.step_up.method"),
        Some("cli_confirm")
    );
}

/// One refusal case's journal and store, and the request it asks.
type Build = Box<dyn Fn(&mut Fixture) -> common::Asked>;

/// DEC-923 item 3: each condition broken alone refuses with its own code and commits nothing.
#[test]
fn every_other_live_deployment_is_refused_and_commits_nothing() {
    let unrecorded = |fx: &mut Fixture| {
        let version = store(fx, live_mandate(&[]));
        fx.ask_in("live", AGENT, &request(SPY, "1", &version))
    };
    let cases: Vec<(&str, Build)> = vec![
        ("live_scope_unknown", Box::new(unrecorded)),
        (
            "live_scope_unknown",
            Box::new(|fx| {
                let version = store(fx, live_mandate(&[]));
                connection_of(fx, "conn_rh_live_02", "robinhood", "live", OWNER);
                fx.ask_in("live", AGENT, &request(SPY, "1", &version))
            }),
        ),
        (
            "live_scope_unknown",
            Box::new(|fx| {
                let asked = dec_529(fx);
                revoked(fx);
                asked
            }),
        ),
        (
            "live_scope_unknown",
            Box::new(|fx| {
                connection(fx, "robinhood", "live", OWNER);
                let version = format!("sha256:{}", "5".repeat(64));
                fx.ask_in("live", AGENT, &request(SPY, "1", &version))
            }),
        ),
        (
            "live_scope_unknown",
            Box::new(|fx| {
                let version = store(fx, live_mandate(&[]));
                connection(fx, "robinhood", "paper", OWNER);
                fx.ask_in("live", AGENT, &request(SPY, "1", &version))
            }),
        ),
        (
            "live_not_robinhood",
            Box::new(|fx| {
                let version = store(fx, live_mandate(&[]));
                connection(fx, "alpaca", "live", OWNER);
                fx.ask_in("live", AGENT, &request(SPY, "1", &version))
            }),
        ),
        (
            "live_mandate_cap_above_limit",
            Box::new(|fx| {
                let edit = [(r#""max_order_usd":"100""#, r#""max_order_usd":"300""#)];
                let version = store(fx, live_mandate(&edit));
                connection(fx, "robinhood", "live", OWNER);
                fx.ask_in("live", AGENT, &request(SPY, "1", &version))
            }),
        ),
        (
            "live_not_one_instrument",
            Box::new(|fx| {
                let edit = [(
                    r#""symbol":"SPY"}]"#,
                    r#""symbol":"SPY"},{"asset_class":"us_equity","asset_id":"c0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"QQQ"}]"#,
                )];
                let version = store(fx, live_mandate(&edit));
                connection(fx, "robinhood", "live", OWNER);
                fx.ask_in("live", AGENT, &request(SPY, "1", &version))
            }),
        ),
        (
            "live_not_equity",
            Box::new(|fx| {
                let version = store(fx, live_mandate(&[]));
                connection(fx, "robinhood", "live", OWNER);
                let mut crypto = request(SPY, "1", &version);
                crypto.bound.asset_class = AssetClass::Crypto;
                fx.ask_in("live", AGENT, &crypto)
            }),
        ),
        (
            "live_other_instrument",
            Box::new(|fx| {
                let version = store(fx, live_mandate(&[]));
                connection(fx, "robinhood", "live", OWNER);
                fx.ask_in("live", AGENT, &request("AAPL", "1", &version))
            }),
        ),
        (
            "live_order_above_limit",
            Box::new(|fx| {
                let version = store(fx, live_mandate(&[]));
                connection(fx, "robinhood", "live", OWNER);
                fx.ask_in("live", AGENT, &request(SPY, "2", &version))
            }),
        ),
        (
            "live_not_owner",
            Box::new(|fx| {
                let version = store(fx, live_mandate(&[]));
                connection(fx, "robinhood", "live", "user-other");
                fx.ask_in("live", AGENT, &request(SPY, "1", &version))
            }),
        ),
    ];
    for (reason, build) in cases {
        let mut fx = Fixture::new();
        let asked = build(&mut fx);
        refused_with(&mut fx, &asked, reason);
    }
}

/// PX-7 and DEC-923 item 4: a skip on a live stream needs no scope, and paper needs none either.
#[test]
fn a_skip_and_a_paper_grant_are_not_held() {
    let mut fx = Fixture::new();
    let version = store(&mut fx, live_mandate(&[]));
    let asked = fx.ask_in("live", AGENT, &request(SPY, "1", &version));
    skip(
        &mut fx.journal,
        &mut FixedIds::default(),
        &common::owner(),
        AGENT,
        &asked.approval,
        at(ASKED_AT + 5),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(responses(&fx).len(), 1);

    let mut paper = Fixture::new();
    let asked = paper.ask(AGENT, "10", ASKED_AT);
    granting(&mut paper, &asked).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(responses(&paper).len(), 1);
}
