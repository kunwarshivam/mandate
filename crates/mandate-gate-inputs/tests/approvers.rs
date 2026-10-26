//! The gate inputs' reading of the control stream into `mandate_identity::approvers` (DEC-953,
//! DEC-2384): each record's sequence, type, event time and payload reach the resolution, the
//! approver references resolve as their texts, a fresh read narrows the admitted set, and DEC-923's
//! live `cli_confirm` scope reads the connection's latest record. The resolution's own cases are
//! `mandate-identity`'s `tests/approvers.rs`; these pin what this crate hands it.

use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_gate_inputs::approvers::{ApproverSource, Members, live_cli_confirm, members, resolve};
use mandate_gate_inputs::control::{ControlRecord, Origin};
use mandate_num::Usd;
use mandate_runtime::{LiveBroker, LiveScope, OpaqueUser};
use mandate_spec::Mandate;
use mandate_spec::document::ApproverRef;

const MANDATE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/mandate.json");
const FOUNDER: &str = "user_founder";

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn mandate() -> Mandate {
    Mandate::parse(&json(MANDATE)).unwrap()
}

fn connection() -> String {
    mandate().connection_id.as_str().to_owned()
}

/// The CLI's version-2 `ConnectionEstablished` of the mandate's connection on `broker` in
/// `environment`, recorded by `user`.
fn established(seq: u64, broker: &str, environment: &str, user: &str) -> ControlRecord {
    let connection = connection();
    ControlRecord {
        seq,
        event_time: None,
        event_type: "ConnectionEstablished".to_owned(),
        payload: json(&format!(
            r#"{{"account_ref":"acct_spy","broker":"{broker}","connection_id":"{connection}","environment":"{environment}","scopes":["trading"],"user":"{user}"}}"#
        )),
        origin: Origin {
            stream_id: "ctl:ws_spy".to_owned(),
            environment: Environment::Live,
        },
    }
}

fn references(texts: &[&str]) -> Vec<ApproverRef> {
    texts
        .iter()
        .map(|text| ApproverRef::parse(text).unwrap())
        .collect()
}

fn users(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

/// The connection's establishment reaches the resolution, which names its user the owner; the
/// owner's role and own `user:` references resolve to the owner, another user's to no one.
#[test]
fn the_control_records_resolve_the_owner() {
    let records = [established(1, "alpaca", "paper", FOUNDER)];
    let owner = members(&records, &connection());
    assert_eq!(owner, Members::Owner(FOUNDER.to_owned()));
    assert_eq!(
        resolve(&references(&["role:approver"]), &owner),
        users(&[FOUNDER])
    );
    assert_eq!(
        resolve(&references(&["user:user_founder"]), &owner),
        users(&[FOUNDER])
    );
    assert_eq!(
        resolve(&references(&["user:user_other"]), &owner),
        users(&[])
    );
    assert_eq!(members(&records, "conn_other"), Members::Unread);
}

/// A fresh read narrows the admitted approvers to those it still resolves, and adds no one.
#[test]
fn a_fresh_read_narrows_the_admitted_approvers() {
    let source = ApproverSource {
        references: references(&["role:approver"]),
        connection: connection(),
    };
    let admitted = users(&[FOUNDER, "user_other"]);
    let still = [established(1, "alpaca", "paper", FOUNDER)];
    assert_eq!(source.current(&admitted, &still), users(&[FOUNDER]));
    let handed = [established(1, "alpaca", "paper", "user_third")];
    assert_eq!(source.current(&admitted, &handed), users(&[]));
}

/// A live run whose connection's latest record is a live establishment by the owner gives the
/// scope from that record and the mandate; a paper record, or a paper run, gives none.
#[test]
fn a_live_record_by_the_owner_gives_the_live_scope() {
    let mandate = mandate();
    let live = [established(1, "robinhood", "live", FOUNDER)];
    let expected = LiveScope {
        broker: LiveBroker::Robinhood,
        max_order_usd: Usd::parse(mandate.risk.max_order_usd.as_str()).unwrap(),
        pinned_instruments: mandate
            .universe
            .pinned_instruments
            .iter()
            .map(|instrument| instrument.asset_id.as_str().to_owned())
            .collect(),
        workspace_owner: OpaqueUser(FOUNDER.to_owned()),
    };
    assert_eq!(
        live_cli_confirm(&live, &mandate, Environment::Live),
        Some(expected)
    );
    let other = [established(1, "alpaca", "live", FOUNDER)];
    assert_eq!(
        live_cli_confirm(&other, &mandate, Environment::Live).map(|scope| scope.broker),
        Some(LiveBroker::Other)
    );
    let paper = [established(1, "robinhood", "paper", FOUNDER)];
    assert_eq!(live_cli_confirm(&paper, &mandate, Environment::Live), None);
    assert_eq!(live_cli_confirm(&live, &mandate, Environment::Paper), None);
}
