//! The deployment's order plan over the journaled fold (DEC-2378 item 10), on the cell's demo
//! mandate: an opening is sized only from journaled facts, with the ask taken as the bid, only
//! while the agent is flat with nothing working, and only in the regular session outside its
//! close window. Any missing fact proposes nothing.

use std::collections::BTreeMap;

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::{Digest, Value};
use mandate_daemon::production::{Facts, Plan};
use mandate_gate_context::Freshness;
use mandate_journal::StoredEvent;
use mandate_num::{Conviction, Price, Qty, Unit};
use mandate_runtime::{
    ApprovalSettings, Autonomy, MandateView, ModelDirection, ModelOutput, OrderPlan, Proposal,
    Purpose, RiskClock, SignalInputs,
};
use mandate_spec::document::Mandate;
use mandate_time::UtcNanos;

const AGENT: &str = "demo-spy";
const CLOCK: &str = "clock:ws1";
const CONTROL: &str = "ctl:ws1";
const ACCOUNT: &str = "acct:ws1:01J8Z3MACCOUNTREF000000001";
const SPY: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const MODEL: &str = "quant.ma_crossover";
const HASH: &str = "cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d";
const FRESH: Freshness = Freshness {
    mark_max_age_s: 60,
    snapshot_max_age_s: 3600,
};
/// Monday 2026-10-26 15:00 UTC, 11:00 in New York: the regular session.
const REGULAR: &str = "2026-10-26T15:00:00.000000000Z";
/// 15:55 in New York: the regular session's close window.
const CLOSING: &str = "2026-10-26T19:55:00.000000000Z";
/// 09:00 in New York: the pre-market session.
const PRE_MARKET: &str = "2026-10-26T13:00:00.000000000Z";

fn at(stamp: &str) -> UtcNanos {
    UtcNanos::parse(stamp).unwrap()
}

struct Rows(Vec<StoredEvent>);

impl Rows {
    fn push(&mut self, stream: &str, event_type: &str, payload: &str) {
        let seq =
            u64::try_from(self.0.iter().filter(|r| r.stream_id == stream).count()).unwrap() + 1;
        let event_id = format!("01J8Z3M{}{seq:018}", stream.len());
        let body = format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":1,"event_time":"{REGULAR}","clock_source":"risk","causation_id":null,"correlation_id":null,"actor":{{"kind":"system","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
        );
        self.0.push(StoredEvent {
            stream_id: stream.into(),
            seq,
            event_id,
            event_type: event_type.into(),
            schema_version: 1,
            environment: "paper".into(),
            recorded_at: REGULAR.into(),
            prev_hash: Digest::of(b"prev"),
            hash: Digest::of(body.as_bytes()),
            body: body.into_bytes(),
        });
    }
}

fn document() -> Value {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/cell/demo/mandate.json"
    ))
    .unwrap();
    mandate_canon::parse(&bytes).unwrap()
}

/// The demo deployment with every fact an opening needs at `now`: the deployment, the model's
/// registration, a scheduler tick, a snapshot, and X's fixture quote as a sane mark.
fn journaled(now: &str) -> Rows {
    let mut rows = Rows(Vec::new());
    rows.push(
        CONTROL,
        "AgentDeployed",
        &format!(r#"{{"agent_id":"{AGENT}","mandate_version":"sha256:{HASH}"}}"#),
    );
    rows.push(
        CONTROL,
        "ConfigSnapshotRegistered",
        &format!(
            r#"{{"kind":"model_version","content_hash":"sha256:{HASH}","model_id":"{MODEL}","model_version":"1.0.0","params":[],"admits_instruments":false}}"#
        ),
    );
    rows.push(CLOCK, "ClockAdvanced", &format!(r#"{{"clock":"{now}"}}"#));
    rows.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","risk_clock":"{now}"}}"#
        ),
    );
    rows.push(
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{SPY}","price":"100.05","source":"quote","feed":"fixture","bid":"99.95","sane":true,"session":"regular","risk_clock":"{now}"}}"#
        ),
    );
    rows
}

fn plan_over(rows: &Rows) -> Plan {
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    Plan::new(facts, Mandate::parse(&document()).unwrap(), FRESH)
}

fn view() -> MandateView {
    MandateView {
        version: format!("sha256:{HASH}"),
        working_universe: Default::default(),
        restricted_instruments: Default::default(),
        approval: ApprovalSettings {
            approvers: Default::default(),
            author: "user-owner".into(),
            timeout_s: 600,
            environment: mandate_journal::Environment::Paper,
            live_cli_confirm: None,
        },
    }
}

/// ma_crossover's long output on SPY, fresh at `now`.
fn inputs(now: &str) -> SignalInputs {
    let secs = at(now).secs();
    let output = ModelOutput {
        model_id: MODEL.into(),
        model_version: "1.0.0".into(),
        content_hash: Digest::from_hex(HASH).unwrap(),
        instrument_id: InstrumentId::new(SPY).unwrap(),
        as_of: RiskClock::from_secs(secs - 60),
        expires_at: RiskClock::from_secs(secs + 3600),
        direction: ModelDirection::Long,
        conviction: Conviction::parse("1").unwrap(),
        confidence: Unit::parse("1").unwrap(),
        horizon_s: 86_400,
        thesis_ref: None,
        evidence: Vec::new(),
        invalidation: None,
        thesis_id: None,
        lineage_id: None,
        ignored: None,
    };
    SignalInputs {
        outputs: BTreeMap::from([(
            MODEL.to_owned(),
            BTreeMap::from([(InstrumentId::new(SPY).unwrap(), output)]),
        )]),
        output_events: BTreeMap::new(),
        now: RiskClock::from_secs(secs),
    }
}

fn proposed(rows: &Rows, now: &str) -> Option<Proposal> {
    plan_over(rows).plan(&view(), &inputs(now))
}

#[test]
fn a_flat_agent_with_every_fact_journaled_proposes_an_opening_never_above_the_bid() {
    let proposal = proposed(&journaled(REGULAR), REGULAR).unwrap();
    assert_eq!(proposal.side, Side::Buy);
    assert_eq!(proposal.purpose, Purpose::Open);
    assert_eq!(proposal.instrument.as_str(), SPY);
    assert!(
        proposal.limit <= Price::parse("99.95").unwrap(),
        "{proposal:?}"
    );
    let cost = proposal.qty.notional(proposal.limit).unwrap();
    assert!(
        cost <= mandate_num::Usd::parse("300").unwrap(),
        "{proposal:?}"
    );
    assert!(proposal.qty >= Qty::parse("1").unwrap(), "{proposal:?}");
    let execution = proposal.execution.unwrap();
    assert!(execution.protection_required);
    let protection = execution.protection.unwrap();
    assert_eq!(protection.stop, Price::parse("94.96").unwrap());
    assert_eq!(
        protection.take_profit,
        Some(Price::parse("109.95").unwrap())
    );
}

#[test]
fn the_demo_mandate_asks_the_owner_about_the_opening() {
    let rows = journaled(REGULAR);
    let plan = plan_over(&rows);
    let proposal = plan.plan(&view(), &inputs(REGULAR)).unwrap();
    let classified = plan.classify(&view(), &proposal);
    assert_eq!(classified.autonomy, Autonomy::Ask, "{classified:?}");
}

/// A proposal in an instrument outside the universe, or with a score that is not one, has no
/// facts to classify it on and is denied; the classification is of the proposal's own figures,
/// and its caps are the gate's (`AGENTS.md` rule 1).
#[test]
fn a_proposal_with_no_facts_to_classify_it_on_is_denied() {
    let rows = journaled(REGULAR);
    let plan = plan_over(&rows);
    let proposal = plan.plan(&view(), &inputs(REGULAR)).unwrap();
    let mut elsewhere = proposal.clone();
    elsewhere.instrument = InstrumentId::new("11111111-2222-4333-8444-555555555555").unwrap();
    assert_eq!(plan.classify(&view(), &elsewhere).autonomy, Autonomy::Deny);
    let mut unscored = proposal.clone();
    unscored.combined_score = Value::Null;
    assert_eq!(plan.classify(&view(), &unscored).autonomy, Autonomy::Deny);
    let stale = plan_over(&journaled(CLOSING));
    assert_eq!(stale.classify(&view(), &proposal).autonomy, Autonomy::Deny);
}

/// Trading-domain spec §9: the close window is the regular session's last 10 minutes, 15:50 to
/// 16:00 in New York, and admits no opening from its first second.
#[test]
fn no_opening_outside_the_regular_session_or_in_its_close_window() {
    assert!(proposed(&journaled(CLOSING), CLOSING).is_none());
    let window_opens = "2026-10-26T19:50:00.000000000Z";
    assert!(proposed(&journaled(window_opens), window_opens).is_none());
    let just_before = "2026-10-26T19:49:59.000000000Z";
    assert!(proposed(&journaled(just_before), just_before).is_some());
    assert!(proposed(&journaled(PRE_MARKET), PRE_MARKET).is_none());
}

#[test]
fn no_opening_while_the_agent_holds_the_instrument_or_has_an_order_working_in_it() {
    let mut rows = journaled(REGULAR);
    rows.push(
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{REGULAR}"}}"#),
    );
    rows.push(
        ACCOUNT,
        "OrderSubmitted",
        &format!(r#"{{"client_order_id":"o1","instrument_id":"{SPY}","side":"buy"}}"#),
    );
    assert!(proposed(&rows, REGULAR).is_none(), "working");
    rows.push(
        ACCOUNT,
        "FillApplied",
        &format!(
            r#"{{"fill_id":"f1","client_order_id":"o1","instrument_id":"{SPY}","side":"buy","qty_gross":"1","price":"99.95","risk_clock":"{REGULAR}"}}"#
        ),
    );
    rows.push(
        ACCOUNT,
        "OrderStateChanged",
        r#"{"client_order_id":"o1","state":"filled","replaced_by":null,"ignored":false}"#,
    );
    assert!(proposed(&rows, REGULAR).is_none(), "held");
}

#[test]
fn a_missing_fact_proposes_nothing() {
    let full = journaled(REGULAR);
    for dropped in [
        "MarkUpdated",
        "AccountSnapshotRecorded",
        "ClockAdvanced",
        "AgentDeployed",
        "ConfigSnapshotRegistered",
    ] {
        let rows = Rows(
            full.0
                .iter()
                .filter(|r| r.event_type != dropped)
                .cloned()
                .collect(),
        );
        assert!(proposed(&rows, REGULAR).is_none(), "without {dropped}");
    }
}

#[test]
fn a_mark_that_is_not_sane_proposes_nothing() {
    let mut rows = journaled(REGULAR);
    rows.push(
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{SPY}","price":"100.05","source":"quote","feed":"fixture","bid":"99.95","sane":false,"session":"regular","risk_clock":"{REGULAR}"}}"#
        ),
    );
    assert!(proposed(&rows, REGULAR).is_none());
}
