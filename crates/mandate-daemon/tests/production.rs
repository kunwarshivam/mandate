//! The deployment's production ports over the journal (DEC-2378 S2): the shared fold the process
//! feeds as it tails, and the flatten planner that sells the agent's sub-ledger from it whether
//! or not the facts an opening needs are there (`AGENTS.md` rule 13).

use std::collections::BTreeMap;

use mandate_accounting::InstrumentId;
use mandate_canon::Digest;
use mandate_daemon::production::{Facts, Flatten};
use mandate_domain::AssetClass;
use mandate_journal::StoredEvent;
use mandate_num::Qty;
use mandate_runtime::{
    FlattenLeg, FlattenPlanner, FlattenRequest, Initiator, OwnerConfirmation, Purpose,
};

const AGENT: &str = "agent-a";
const CLOCK: &str = "clock:ws1";
const ACCOUNT: &str = "acct:ws1:01J8Z3MACCOUNTREF000000001";
const SPY: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const BTC: &str = "5d2c1a3e-0b4f-4c7a-9e21-3f6d8a9b0c11";
const STAMP: &str = "2026-10-16T13:00:00.000000000Z";

fn row(stream: &str, seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01J8Z3MA{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":1,"event_time":"{STAMP}","clock_source":"risk","causation_id":null,"correlation_id":null,"actor":{{"kind":"system","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: stream.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 1,
        environment: "paper".into(),
        recorded_at: STAMP.into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// The account stream's rows: `agent`'s order `order` in `instrument`, and its buy fill of `qty`.
fn bought(rows: &mut Vec<StoredEvent>, agent: &str, order: &str, instrument: &str, qty: &str) {
    let mut push = |event_type: &str, payload: String| {
        let seq = u64::try_from(rows.len()).unwrap() + 1;
        rows.push(row(ACCOUNT, seq, event_type, &payload));
    };
    push(
        "OrderRequestRecorded",
        format!(r#"{{"agent_id":"{agent}","purpose":"open","risk_clock":"{STAMP}"}}"#),
    );
    push(
        "OrderSubmitted",
        format!(r#"{{"client_order_id":"{order}","instrument_id":"{instrument}","side":"buy"}}"#),
    );
    push(
        "FillApplied",
        format!(
            r#"{{"fill_id":"f-{order}","client_order_id":"{order}","instrument_id":"{instrument}","side":"buy","qty_gross":"{qty}","price":"100","risk_clock":"{STAMP}"}}"#
        ),
    );
}

fn classes() -> BTreeMap<String, AssetClass> {
    BTreeMap::from([
        (SPY.to_owned(), AssetClass::UsEquity),
        (BTC.to_owned(), AssetClass::Crypto),
    ])
}

fn facts() -> Facts {
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts
}

fn kill_switch() -> FlattenRequest {
    FlattenRequest {
        initiator: Initiator::RiskLimit,
        instrument: None,
        confirmation: None,
        working_orders: Vec::new(),
    }
}

fn leg(instrument: &str, asset_class: AssetClass, qty: &str) -> FlattenLeg {
    FlattenLeg {
        instrument: InstrumentId::new(instrument).unwrap(),
        asset_class,
        qty: Qty::parse(qty).unwrap(),
        deferred_to_regular_session: false,
    }
}

/// The coordinator's pin on S2: with no clock, no snapshot, no mark and no mandate journaled, a
/// kill switch still sells everything the agent's own fills hold.
#[test]
fn with_no_opening_facts_a_kill_switch_sells_the_whole_sub_ledger() {
    let facts = facts();
    let mut rows = Vec::new();
    bought(&mut rows, AGENT, "o1", SPY, "3");
    bought(&mut rows, AGENT, "o2", BTC, "0.25");
    bought(&mut rows, "agent-b", "o3", SPY, "7");
    facts.feed(&rows);
    let plan = Flatten::new(facts, classes()).plan(&kill_switch());
    assert_eq!(
        plan.sells,
        vec![
            leg(BTC, AssetClass::Crypto, "0.25"),
            leg(SPY, AssetClass::UsEquity, "3")
        ]
    );
    assert_eq!(plan.purpose, Purpose::RiskExit);
    assert!(plan.confirmation.is_none());
}

#[test]
fn a_kill_switch_cancels_every_working_order_of_the_agent_and_no_other() {
    let facts = facts();
    let mut rows = Vec::new();
    bought(&mut rows, AGENT, "o1", SPY, "3");
    bought(&mut rows, AGENT, "o2", BTC, "1");
    bought(&mut rows, "agent-b", "o3", SPY, "7");
    let seq = u64::try_from(rows.len()).unwrap() + 1;
    rows.push(row(
        ACCOUNT,
        seq,
        "OrderStateChanged",
        r#"{"client_order_id":"o2","state":"filled","replaced_by":null,"ignored":false}"#,
    ));
    facts.feed(&rows);
    let plan = Flatten::new(facts, classes()).plan(&kill_switch());
    assert_eq!(plan.cancel_client_order_ids, vec!["o1".to_owned()]);
}

#[test]
fn an_owner_exit_sells_and_cancels_only_its_instrument_with_the_owners_confirmation() {
    let facts = facts();
    let mut rows = Vec::new();
    bought(&mut rows, AGENT, "o1", SPY, "3");
    bought(&mut rows, AGENT, "o2", BTC, "1");
    facts.feed(&rows);
    let confirmation = OwnerConfirmation {
        bid: mandate_num::Price::parse("99").unwrap(),
        bid_size: Qty::parse("100").unwrap(),
        floor: mandate_num::Price::parse("95").unwrap(),
        user: "user-owner".into(),
        step_up: "assertion-1".into(),
    };
    let request = FlattenRequest {
        initiator: Initiator::Owner,
        instrument: Some(InstrumentId::new(SPY).unwrap()),
        confirmation: Some(confirmation.clone()),
        working_orders: Vec::new(),
    };
    let plan = Flatten::new(facts, classes()).plan(&request);
    assert_eq!(plan.sells, vec![leg(SPY, AssetClass::UsEquity, "3")]);
    assert_eq!(plan.cancel_client_order_ids, vec!["o1".to_owned()]);
    assert_eq!(plan.purpose, Purpose::OwnerExit);
    assert_eq!(plan.confirmation, Some(confirmation));
}

#[test]
fn rows_fed_again_count_once_and_rows_before_the_account_was_known_still_count() {
    let facts = Facts::new(CLOCK, AGENT);
    let mut rows = Vec::new();
    bought(&mut rows, AGENT, "o1", SPY, "3");
    facts.feed(&rows);
    facts.follow(ACCOUNT);
    facts.feed(&rows);
    facts.feed(&rows);
    let plan = Flatten::new(facts, classes()).plan(&kill_switch());
    assert_eq!(plan.sells, vec![leg(SPY, AssetClass::UsEquity, "3")]);
}

/// A row whose body does not parse is fed as a row with no payload: one of a type the fold reads
/// holds every later opening, and the flatten still sells what the readable rows hold.
#[test]
fn an_unreadable_row_holds_openings_and_never_the_flatten() {
    let facts = facts();
    let mut rows = Vec::new();
    bought(&mut rows, AGENT, "o1", SPY, "3");
    let mut broken = row(ACCOUNT, 4, "MarkUpdated", "{}");
    broken.body = b"not json".to_vec();
    rows.push(broken);
    facts.feed(&rows);
    let held = facts.read(|journaled| {
        journaled.facts(
            SPY,
            mandate_gate_context::Freshness {
                mark_max_age_s: 60,
                snapshot_max_age_s: 60,
            },
        )
    });
    assert_eq!(held, Some(Err(mandate_gate_context::Held::Unreadable)));
    let plan = Flatten::new(facts, classes()).plan(&kill_switch());
    assert_eq!(plan.sells, vec![leg(SPY, AssetClass::UsEquity, "3")]);
}

/// The mandate's universe names every instrument the agent can hold. One it does not name has no
/// asset class to sell it under, so the plan leaves it to the executor, which caps every sell at
/// what the account holds, and still sells the rest.
#[test]
fn a_holding_outside_the_universe_does_not_hold_the_rest_of_the_flatten() {
    let facts = facts();
    let mut rows = Vec::new();
    bought(
        &mut rows,
        AGENT,
        "o1",
        "11111111-2222-4333-8444-555555555555",
        "2",
    );
    bought(&mut rows, AGENT, "o2", SPY, "3");
    facts.feed(&rows);
    let plan = Flatten::new(facts, classes()).plan(&kill_switch());
    assert_eq!(plan.sells, vec![leg(SPY, AssetClass::UsEquity, "3")]);
}

#[test]
fn before_the_account_is_known_the_flatten_has_nothing_to_sell() {
    let facts = Facts::new(CLOCK, AGENT);
    let plan = Flatten::new(facts, classes()).plan(&kill_switch());
    assert!(plan.sells.is_empty());
    assert!(plan.cancel_client_order_ids.is_empty());
}

/// S3a: a resolved paper deployment's wiring sells its pinned universe's sub-ledger under each
/// instrument's class from the stored mandate; a holding outside it has no class to sell under.
/// The plan and the gate stay the fixture's deny-all ones until S2b wires them.
#[test]
fn a_resolved_deployment_is_wired_with_the_fold_and_the_flatten_over_its_universe() {
    let document = mandate_canon::parse(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/cell/demo/mandate.json"
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        mandate_daemon::production::classes(&document),
        BTreeMap::from([(SPY.to_owned(), AssetClass::UsEquity)])
    );
    let wiring = mandate_daemon::production::wired(
        mandate_daemon::fixture::wiring("user-owner"),
        &document,
        CLOCK,
        AGENT,
    );
    let facts = wiring.facts.clone().unwrap();
    facts.follow(ACCOUNT);
    let mut rows = Vec::new();
    bought(&mut rows, AGENT, "o1", SPY, "2");
    bought(&mut rows, AGENT, "o2", BTC, "1");
    facts.feed(&rows);
    let plan = wiring.flatten.plan(&kill_switch());
    assert_eq!(plan.sells, vec![leg(SPY, AssetClass::UsEquity, "2")]);
}

#[test]
fn a_document_that_does_not_parse_has_no_classes() {
    let document = mandate_canon::parse(br#"{"universe":{}}"#).unwrap();
    assert!(mandate_daemon::production::classes(&document).is_empty());
}
