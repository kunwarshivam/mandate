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
    ApprovalSettings, Autonomy, Classified, DecisionClip, EventId, MandateView, ModelDirection,
    ModelOutput, OrderPlan, OwnerRequest, Proposal, Purpose, Requester, RiskClock, SignalInputs,
};
use mandate_spec::document::Mandate;
use mandate_time::UtcNanos;

const AGENT: &str = "demo-spy";
const CLOCK: &str = "clock:ws1";
const CONTROL: &str = "ctl:ws1";
const ACCOUNT: &str = "acct:ws1:01J8Z3MACCOUNTREF000000001";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
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
        ended_theses: Default::default(),
        working_exits: Default::default(),
        thesis_classes: Default::default(),
    }
}

/// The demo mandate with `default: auto` and one rule asking about any opening once the risk
/// day's buys, this order included, pass `above` dollars: a rule on a fact the plan cannot see
/// (mandate spec §6.3).
fn auto_with_a_daily_buy_rule(above: &str) -> Value {
    let mut document: serde_json::Value =
        serde_json::from_slice(&mandate_canon::to_canonical(&document())).unwrap();
    document["autonomy"]["default"] = serde_json::json!("auto");
    document["autonomy"]["rules"] = serde_json::json!([{
        "id": "daily_buys", "then": "ask",
        "when": {"field": "bought_today_usd", "op": "gt", "value": above},
    }]);
    mandate_canon::parse(&serde_json::to_vec(&document).unwrap()).unwrap()
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
    let cap = document()
        .get("risk")
        .and_then(|risk| risk.get("max_order_usd"))
        .and_then(Value::as_str)
        .map(|cap| mandate_num::Usd::parse(cap).unwrap())
        .unwrap();
    assert!(cost <= cap, "{proposal:?} within the envelope's order cap");
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
    unscored.combined_score = Value::Str("not a score".into());
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

/// An `auto` mandate whose ASK rule reads what the agent bought today, after a buy and a sell
/// that left it flat: the rule's threshold is $100 above the next opening's own cost, so only
/// the earlier $199.90 buy would carry it over. The plan sees none of those buys, so the rule
/// cannot fire on its facts, and the opening is asked under the policy overlay rather than
/// placed as the day's first (§6.2 step 5c, DEC-2378 item 10). Repeated round trips cannot split
/// orders past the rule.
#[test]
fn an_auto_mandate_is_asked_about_an_opening_after_a_round_trip_it_cannot_see() {
    let mut rows = journaled(REGULAR);
    for (order, side) in [("o1", "buy"), ("o2", "sell")] {
        rows.push(
            ACCOUNT,
            "OrderRequestRecorded",
            &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{REGULAR}"}}"#),
        );
        rows.push(
            ACCOUNT,
            "OrderSubmitted",
            &format!(r#"{{"client_order_id":"{order}","instrument_id":"{SPY}","side":"{side}"}}"#),
        );
        rows.push(
            ACCOUNT,
            "FillApplied",
            &format!(
                r#"{{"fill_id":"f-{order}","client_order_id":"{order}","instrument_id":"{SPY}","side":"{side}","qty_gross":"2","price":"99.95","risk_clock":"{REGULAR}"}}"#
            ),
        );
        rows.push(
            ACCOUNT,
            "OrderStateChanged",
            &format!(
                r#"{{"client_order_id":"{order}","state":"filled","replaced_by":null,"ignored":false}}"#
            ),
        );
    }
    let sized = proposed(&rows, REGULAR).expect("flat again, so an opening is proposed");
    let above = sized
        .qty
        .notional(sized.limit)
        .unwrap()
        .checked_add(mandate_num::Usd::parse("100").unwrap())
        .unwrap();
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    let plan = Plan::new(
        facts,
        Mandate::parse(&auto_with_a_daily_buy_rule(&above.to_string())).unwrap(),
        FRESH,
    );
    let proposal = plan
        .plan(&view(), &inputs(REGULAR))
        .expect("flat again, so an opening is proposed");
    assert_eq!(
        plan.classify(&view(), &proposal),
        Classified {
            autonomy: Autonomy::Ask,
            decided_by: Some("policy_overlay".into()),
        },
        "never auto: the overlay changed the decision, so it names itself"
    );
}

/// The overlay only tightens: an `auto` opening or increase becomes `ask`, while an `ask` or a
/// `deny` keeps its own label and an exit is never touched (`AGENTS.md` rule 13).
#[test]
fn the_overlay_asks_only_about_an_auto_opening_or_increase() {
    let rows = journaled(REGULAR);
    let proposal = plan_over(&rows).plan(&view(), &inputs(REGULAR)).unwrap();
    let decided = |autonomy, by: &str| Classified {
        autonomy,
        decided_by: Some(by.into()),
    };
    let asked = decided(Autonomy::Ask, "policy_overlay");
    for purpose in [Purpose::Open, Purpose::Increase] {
        let mut adds = proposal.clone();
        adds.purpose = purpose;
        assert_eq!(
            mandate_ports::auto_not_allowed(decided(Autonomy::Auto, "default"), &adds),
            asked,
            "{purpose:?}"
        );
        for kept in [
            decided(Autonomy::Ask, "rule:large"),
            decided(Autonomy::Deny, "rule:never"),
        ] {
            assert_eq!(
                mandate_ports::auto_not_allowed(kept.clone(), &adds),
                kept,
                "{purpose:?}"
            );
        }
    }
    for purpose in [
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ] {
        let mut exit = proposal.clone();
        exit.purpose = purpose;
        let auto = decided(Autonomy::Auto, "builtin_risk_reducing");
        assert_eq!(
            mandate_ports::auto_not_allowed(auto.clone(), &exit),
            auto,
            "{purpose:?}"
        );
    }
}

/// The demo mandate with one more rule, `rule`, ahead of none: its default stays `ask`.
fn with_rule(rule: serde_json::Value) -> Value {
    let mut document: serde_json::Value =
        serde_json::from_slice(&mandate_canon::to_canonical(&document())).unwrap();
    document["autonomy"]["rules"] = serde_json::json!([rule]);
    mandate_canon::parse(&serde_json::to_vec(&document).unwrap()).unwrap()
}

fn classified_under(rule: serde_json::Value) -> Classified {
    let rows = journaled(REGULAR);
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    let plan = Plan::new(facts, Mandate::parse(&with_rule(rule)).unwrap(), FRESH);
    let proposal = plan.plan(&view(), &inputs(REGULAR)).expect("an opening");
    plan.classify(&view(), &proposal)
}

/// A `deny` rule on a fact the plan cannot see is never offered to an approver: the plan reads
/// that fact as zero, so the rule cannot fire on it, and every opening is denied under the policy
/// overlay instead, however deep the field sits in the rule's condition. A `deny` rule on a fact
/// the plan does see is judged on it as usual (`AGENTS.md` rule 1, DEC-2378 item 10).
#[test]
fn a_deny_rule_on_a_fact_the_plan_cannot_see_denies_every_opening() {
    let overlay = Classified {
        autonomy: Autonomy::Deny,
        decided_by: Some("policy_overlay".into()),
    };
    for (field, value) in [
        ("bought_today_usd", serde_json::json!("1000")),
        ("drawdown", serde_json::json!("0.05")),
        ("daily_pnl_fraction", serde_json::json!("0.05")),
        ("gross_usd_after", serde_json::json!("1000")),
        ("thesis_confidence", serde_json::json!("0.5")),
    ] {
        let leaf = serde_json::json!({"field": field, "op": "gt", "value": value});
        assert_eq!(
            classified_under(serde_json::json!({"id": "cap", "then": "deny", "when": leaf})),
            overlay,
            "{field}"
        );
        let nested = serde_json::json!({"all": [
            {"field": "purpose", "op": "in", "value": ["open", "increase"]},
            {"not": {"any": [leaf]}},
        ]});
        assert_eq!(
            classified_under(serde_json::json!({"id": "cap", "then": "deny", "when": nested})),
            overlay,
            "{field}, nested"
        );
    }
    for field in ["first_trade_in_instrument", "new_instrument"] {
        assert_eq!(
            classified_under(serde_json::json!({"id": "flag", "then": "deny", "when":
                {"field": field, "op": "eq", "value": true}})),
            overlay,
            "{field}"
        );
    }
    let seen = classified_under(serde_json::json!({"id": "big", "then": "deny", "when":
        {"field": "order_usd", "op": "gt", "value": "100000"}}));
    assert_eq!(
        seen,
        Classified {
            autonomy: Autonomy::Ask,
            decided_by: Some("default".into()),
        },
        "order_usd is the plan's own figure: the rule does not match, and the default asks"
    );
    let asked = classified_under(serde_json::json!({"id": "daily", "then": "ask", "when":
        {"field": "bought_today_usd", "op": "gt", "value": "100000"}}));
    assert_eq!(
        asked.autonomy,
        Autonomy::Ask,
        "an ask rule on an unseen fact still asks, never denies: {asked:?}"
    );
}

/// The demo's research mandate: an unpinned universe, so its theses can end (V-037).
fn research_document() -> Value {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/cell/demo/research-mandate.json"
    ))
    .unwrap();
    mandate_canon::parse(&bytes).unwrap()
}

fn research_plan(rows: &Rows) -> Plan {
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    Plan::new(facts, Mandate::parse(&research_document()).unwrap(), FRESH)
}

/// `rows` with the agent holding `qty` SPY from one filled buy of its own, journaled at `now`.
fn holding(mut rows: Rows, qty: &str, now: &str) -> Rows {
    rows.push(
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{now}"}}"#),
    );
    rows.push(
        ACCOUNT,
        "OrderSubmitted",
        &format!(r#"{{"client_order_id":"o1","instrument_id":"{SPY}","side":"buy"}}"#),
    );
    rows.push(
        ACCOUNT,
        "FillApplied",
        &format!(
            r#"{{"fill_id":"f1","client_order_id":"o1","instrument_id":"{SPY}","side":"buy","qty_gross":"{qty}","price":"99.95","risk_clock":"{now}"}}"#
        ),
    );
    rows.push(
        ACCOUNT,
        "OrderStateChanged",
        r#"{"client_order_id":"o1","state":"filled","replaced_by":null,"ignored":false}"#,
    );
    rows
}

/// No output at all, SPY's thesis ended, and its class from its thesis record (DEC-2391).
fn ended_inputs(now: &str) -> SignalInputs {
    let spy = InstrumentId::new(SPY).unwrap();
    SignalInputs {
        outputs: BTreeMap::new(),
        output_events: BTreeMap::new(),
        now: RiskClock::from_secs(at(now).secs()),
        ended_theses: [spy.clone()].into(),
        working_exits: Default::default(),
        thesis_classes: [(spy, mandate_accounting::AssetClass::UsEquity)].into(),
    }
}

/// Mandate spec §8.3 step 2, MI-34: a held instrument whose thesis ended is sold whole at the
/// bid, with `exit_origin` `removed_instrument`, built-in AUTO, in every session: whether an
/// equity exit waits for the regular session is the gate's and the executor's, never the plan's.
#[test]
fn a_held_instrument_whose_thesis_ended_is_sold_whole_and_auto_in_every_session() {
    for now in [REGULAR, CLOSING, PRE_MARKET] {
        let rows = holding(journaled(now), "3", now);
        let plan = research_plan(&rows);
        let proposal = plan
            .plan(&view(), &ended_inputs(now))
            .unwrap_or_else(|| panic!("{now}: an exit"));
        assert_eq!(proposal.side, Side::Sell, "{now}");
        assert_eq!(proposal.purpose, Purpose::DiscretionaryExit, "{now}");
        assert_eq!(
            proposal.exit_origin,
            Some(mandate_runtime::ExitOrigin::RemovedInstrument),
            "{now}"
        );
        assert_eq!(proposal.qty, Qty::parse("3").unwrap(), "{now}");
        assert_eq!(proposal.limit, Price::parse("99.95").unwrap(), "{now}");
        assert_eq!(
            proposal.execution.map(|e| e.protection_required),
            Some(false),
            "{now}: an exit carries no protection"
        );
        assert_eq!(
            plan.classify(&view(), &proposal),
            Classified {
                autonomy: Autonomy::Auto,
                decided_by: Some("builtin_risk_reducing".into()),
            },
            "{now}"
        );
    }
}

#[test]
fn nothing_held_no_ended_thesis_or_no_mark_sells_nothing() {
    let none = research_plan(&journaled(REGULAR));
    assert!(
        none.plan(&view(), &ended_inputs(REGULAR)).is_none(),
        "nothing held"
    );
    let held = research_plan(&holding(journaled(REGULAR), "3", REGULAR));
    let current = SignalInputs {
        ended_theses: Default::default(),
        ..ended_inputs(REGULAR)
    };
    assert!(
        held.plan(&view(), &current).is_none(),
        "the thesis is current"
    );
    let classless = SignalInputs {
        thesis_classes: Default::default(),
        ..ended_inputs(REGULAR)
    };
    assert!(
        held.plan(&view(), &classless).is_none(),
        "no thesis record names its class"
    );
    let later = "2026-10-26T17:00:00.000000000Z";
    let mut stale_mark = holding(journaled(REGULAR), "3", REGULAR);
    stale_mark.push(CLOCK, "ClockAdvanced", &format!(r#"{{"clock":"{later}"}}"#));
    stale_mark.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","risk_clock":"{later}"}}"#
        ),
    );
    assert!(
        research_plan(&stale_mark)
            .plan(&view(), &ended_inputs(later))
            .is_none(),
        "a fresh snapshot but no fresh mark to price it at"
    );
}

#[test]
fn a_working_exit_is_taken_off_the_position_and_never_stacked() {
    let plan = research_plan(&holding(journaled(REGULAR), "3", REGULAR));
    let spy = InstrumentId::new(SPY).unwrap();
    let part = SignalInputs {
        working_exits: [(spy.clone(), Qty::parse("1").unwrap())].into(),
        ..ended_inputs(REGULAR)
    };
    assert_eq!(
        plan.plan(&view(), &part).map(|p| p.qty),
        Some(Qty::parse("2").unwrap())
    );
    let covered = SignalInputs {
        working_exits: [(spy, Qty::parse("3").unwrap())].into(),
        ..ended_inputs(REGULAR)
    };
    assert!(plan.plan(&view(), &covered).is_none());
}

#[test]
fn a_pinned_universe_sells_nothing_for_an_ended_thesis_and_never_denies_an_exit() {
    let pinned = plan_over(&holding(journaled(REGULAR), "3", REGULAR));
    assert!(pinned.plan(&view(), &ended_inputs(REGULAR)).is_none());
    let exit = research_plan(&holding(journaled(REGULAR), "3", REGULAR))
        .plan(&view(), &ended_inputs(REGULAR))
        .unwrap();
    assert_eq!(
        pinned.classify(&view(), &exit).autonomy,
        Autonomy::Auto,
        "an exit is never denied or asked (rule 13)"
    );
}

/// Nothing an opening needs holds the exit: an account whose equity fell below the allocation (V-002
/// would refuse the mandate's validation) and a snapshot two hours old still sell at a fresh mark.
#[test]
fn a_drawdown_below_the_allocation_or_an_old_snapshot_never_holds_the_exit() {
    let mut rows = holding(journaled(REGULAR), "3", REGULAR);
    rows.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"500","cash":"200","buying_power":"200","non_marginable_buying_power":"200","risk_clock":"{REGULAR}"}}"#
        ),
    );
    assert_eq!(
        research_plan(&rows)
            .plan(&view(), &ended_inputs(REGULAR))
            .map(|p| p.qty),
        Some(Qty::parse("3").unwrap()),
        "equity 500 under the 1000 allocation"
    );
    let later = "2026-10-26T17:00:00.000000000Z";
    let mut old = holding(journaled(REGULAR), "3", REGULAR);
    old.push(CLOCK, "ClockAdvanced", &format!(r#"{{"clock":"{later}"}}"#));
    old.push(
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{SPY}","price":"101.05","source":"quote","feed":"fixture","bid":"100.95","sane":true,"session":"regular","risk_clock":"{later}"}}"#
        ),
    );
    assert_eq!(
        research_plan(&old)
            .plan(&view(), &ended_inputs(later))
            .map(|p| p.limit),
        Some(Price::parse("100.95").unwrap()),
        "a two-hour-old snapshot, a fresh mark: sold at that bid"
    );
}

/// A crypto thesis's exit is routed as crypto: its class from the record, good-till-cancelled.
#[test]
fn a_crypto_thesis_exit_is_routed_as_crypto() {
    let spy = InstrumentId::new(SPY).unwrap();
    let crypto = SignalInputs {
        thesis_classes: [(spy, mandate_accounting::AssetClass::Crypto)].into(),
        ..ended_inputs(REGULAR)
    };
    let proposal = research_plan(&holding(journaled(REGULAR), "3", REGULAR))
        .plan(&view(), &crypto)
        .unwrap();
    assert_eq!(proposal.asset_class, mandate_accounting::AssetClass::Crypto);
    let execution = proposal.execution.unwrap();
    assert_eq!(
        execution.asset_class,
        mandate_accounting::AssetClass::Crypto
    );
    assert_eq!(execution.tif, mandate_runtime::TimeInForce::Gtc);
    let equity = research_plan(&holding(journaled(REGULAR), "3", REGULAR))
        .plan(&view(), &ended_inputs(REGULAR))
        .unwrap();
    assert_eq!(
        equity.execution.map(|e| e.tif),
        Some(mandate_runtime::TimeInForce::Day)
    );
}

/// The research model's content hash in the demo's research envelope.
const RESEARCH_HASH: &str = "bff754a2e669add9a7928a4b8c71c764cfe61455e361621ae6f6c262af0824cd";

/// The deployment's agent stream, which carries its thesis records.
const AGENT_STREAM: &str = "agent:ws1:demo-spy";

/// Another US equity, which no envelope here pins.
const QQQ: &str = "3a9e1f0c-2b7d-4c55-8e61-0f4d2a6b9c13";

/// A `ThesisProposed` on the agent stream (journal spec §9.4): `thesis` on `instrument`, of
/// `asset_class`, admitted, as of `as_of` and expiring at `expires_at`.
fn thesis(
    rows: &mut Rows,
    thesis: &str,
    instrument: &str,
    asset_class: &str,
    (as_of, expires_at): (&str, &str),
) {
    let hash = Digest::of(b"x").to_hex();
    rows.push(
        AGENT_STREAM,
        "ThesisProposed",
        &format!(
            r#"{{"model_id":"llm.research_agent","model_version":"0.1.0","content_hash":"sha256:{RESEARCH_HASH}","thesis_id":"{thesis}","lineage_id":"{thesis}","revision":0,"predecessor_thesis_id":null,"autopsy_ref":null,"instrument_id":"{instrument}","asset_class":"{asset_class}","direction":"long","as_of":"{as_of}","expires_at":"{expires_at}","horizon_s":86400,"conviction":"0.6","confidence":"0.6","evidence_ref":null,"evidence_sources":["src.newswire"],"corroboration":"independent_source","invalidation":"a guidance cut","allowlist_version":3,"prompt_ref":"sha256:{hash}","response_ref":"sha256:{hash}","admitted":true,"reason":null}}"#
        ),
    );
}

/// A model's `ThesisInvalidated` of `thesis` on `instrument` (journal spec §9.22).
fn invalidated(rows: &mut Rows, thesis: &str, instrument: &str) {
    rows.push(
        AGENT_STREAM,
        "ThesisInvalidated",
        &format!(
            r#"{{"thesis_id":"{thesis}","lineage_id":"{thesis}","instrument_id":"{instrument}","cause":"model","candidate":0,"evidence":["01OBS"],"source":"src.newswire"}}"#
        ),
    );
}

/// The day before `REGULAR` to the day after: a thesis current at `REGULAR`.
const CURRENT: (&str, &str) = (
    "2026-10-25T15:00:00.000000000Z",
    "2026-10-27T15:00:00.000000000Z",
);

/// [`journaled`] with the research model registered too, as the cell's research workspace has it,
/// and no thesis yet.
fn research_registered(now: &str) -> Rows {
    let mut rows = journaled(now);
    rows.push(
        CONTROL,
        "ConfigSnapshotRegistered",
        &format!(
            r#"{{"kind":"model_version","content_hash":"sha256:{RESEARCH_HASH}","model_id":"llm.research_agent","model_version":"0.1.0","params":[],"admits_instruments":true}}"#
        ),
    );
    rows
}

/// [`research_registered`] with SPY's admitted thesis current at `now`, as the cell's research
/// workspace has them.
fn research_journaled(now: &str) -> Rows {
    let mut rows = research_registered(now);
    thesis(
        &mut rows,
        "01J8Z3MTHE5000000000000001",
        SPY,
        "us_equity",
        CURRENT,
    );
    rows
}

/// What the cell's research loop hands the plan at `now`: the research model's `Long` on SPY from
/// its admitted thesis (conviction 0.6) and ma_crossover's `Long` on SPY, that thesis naming SPY a
/// US equity (DEC-2827 keeps a `Long` only on the instrument a surviving thesis holds).
fn working_inputs(now: &str) -> SignalInputs {
    long_on(SPY, now)
}

/// As [`working_inputs`], both `Long`s on `instrument`, its thesis class a US equity.
fn long_on(instrument: &str, now: &str) -> SignalInputs {
    let mut inputs = inputs(now);
    let spy = InstrumentId::new(SPY).unwrap();
    let on = InstrumentId::new(instrument).unwrap();
    let quant = ModelOutput {
        instrument_id: on.clone(),
        ..inputs.outputs[MODEL][&spy].clone()
    };
    let research = ModelOutput {
        model_id: "llm.research_agent".into(),
        model_version: "0.1.0".into(),
        content_hash: Digest::from_hex(RESEARCH_HASH).unwrap(),
        conviction: Conviction::parse("0.6").unwrap(),
        confidence: Unit::parse("0.6").unwrap(),
        thesis_id: Some("01J8Z3MTHE5000000000000001".into()),
        lineage_id: Some("01J8Z3MTHE5000000000000001".into()),
        ..quant.clone()
    };
    inputs.outputs = BTreeMap::from([
        (MODEL.to_owned(), BTreeMap::from([(on.clone(), quant)])),
        (
            "llm.research_agent".to_owned(),
            BTreeMap::from([(on.clone(), research)]),
        ),
    ]);
    inputs.thesis_classes = [(on, mandate_accounting::AssetClass::UsEquity)].into();
    inputs
}

/// E21-2, DEC-2827, the cell's research loop (lane GW's replay of 2026-10-27): an unpinned
/// research mandate opens on its working universe, the instrument the one surviving admitted
/// thesis on its agent stream holds, replayed as the executor replays it (DEC-2819). Both models'
/// `Long` on it is sized from journaled facts as a pinned opening is: a buy at no more than the bid
/// within the envelope's order cap, with its protective stop; and the owner is asked (the research
/// envelope's `default: ask`), never AUTO.
#[test]
fn an_unpinned_mandate_opens_on_its_working_universe_and_asks() {
    let rows = research_journaled(REGULAR);
    let plan = research_plan(&rows);
    let proposal = plan
        .plan(&view(), &working_inputs(REGULAR))
        .expect("an opening on the working universe's SPY");
    assert_eq!(proposal.side, Side::Buy);
    assert_eq!(proposal.purpose, Purpose::Open);
    assert_eq!(proposal.instrument.as_str(), SPY);
    assert!(
        proposal.limit <= Price::parse("99.95").unwrap(),
        "{proposal:?}"
    );
    let cap = research_document()
        .get("risk")
        .and_then(|risk| risk.get("max_order_usd"))
        .and_then(Value::as_str)
        .map(|cap| mandate_num::Usd::parse(cap).unwrap())
        .unwrap();
    let cost = proposal.qty.notional(proposal.limit).unwrap();
    assert!(cost <= cap, "{proposal:?} within the envelope's order cap");
    assert!(proposal.qty >= Qty::parse("1").unwrap(), "{proposal:?}");
    assert!(
        proposal.execution.is_some_and(
            |execution| execution.protection_required && execution.protection.is_some()
        ),
        "an opening carries its protective stop: {proposal:?}"
    );
    let classified = plan.classify(&view(), &proposal);
    assert_eq!(classified.autonomy, Autonomy::Ask, "{classified:?}");
}

/// A pinned mandate is unchanged by the working universe: a `Long` on an instrument it does not
/// pin proposes nothing, whatever a thesis says, and a proposal on it is denied.
#[test]
fn a_pinned_mandate_never_opens_outside_its_pins() {
    let mut rows = journaled(REGULAR);
    rows.push(
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{QQQ}","price":"50.05","source":"quote","feed":"fixture","bid":"49.95","sane":true,"session":"regular","risk_clock":"{REGULAR}"}}"#
        ),
    );
    thesis(
        &mut rows,
        "01J8Z3MTHE5000000000000009",
        QQQ,
        "us_equity",
        CURRENT,
    );
    let plan = plan_over(&rows);
    assert!(
        plan.plan(&view(), &long_on(QQQ, REGULAR)).is_none(),
        "not a pin"
    );
    let spy = plan.plan(&view(), &inputs(REGULAR)).unwrap();
    let elsewhere = Proposal {
        instrument: InstrumentId::new(QQQ).unwrap(),
        ..spy
    };
    assert_eq!(plan.classify(&view(), &elsewhere).autonomy, Autonomy::Deny);
}

/// DEC-2819 item 2, as the executor opens: two surviving theses mean no one working instrument,
/// so no opening, even with a `Long` on one of them.
#[test]
fn two_surviving_theses_open_nothing() {
    let mut rows = research_journaled(REGULAR);
    rows.push(
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{QQQ}","price":"50.05","source":"quote","feed":"fixture","bid":"49.95","sane":true,"session":"regular","risk_clock":"{REGULAR}"}}"#
        ),
    );
    thesis(
        &mut rows,
        "01J8Z3MTHE5000000000000002",
        QQQ,
        "us_equity",
        CURRENT,
    );
    let plan = research_plan(&rows);
    assert!(plan.plan(&view(), &working_inputs(REGULAR)).is_none());
}

/// A `Long` on an instrument a thesis once admitted but no longer holds, its thesis expired or
/// invalidated, proposes nothing.
#[test]
fn a_long_on_an_instrument_whose_thesis_no_longer_holds_opens_nothing() {
    let mut expired = research_registered(REGULAR);
    thesis(
        &mut expired,
        "01J8Z3MTHE5000000000000001",
        SPY,
        "us_equity",
        (
            "2026-10-24T15:00:00.000000000Z",
            "2026-10-25T15:00:00.000000000Z",
        ),
    );
    assert!(
        research_plan(&expired)
            .plan(&view(), &working_inputs(REGULAR))
            .is_none(),
        "expired"
    );
    let mut ended = research_journaled(REGULAR);
    invalidated(&mut ended, "01J8Z3MTHE5000000000000001", SPY);
    assert!(
        research_plan(&ended)
            .plan(&view(), &working_inputs(REGULAR))
            .is_none(),
        "invalidated"
    );
}

/// `AGENTS.md` rule 3: a thesis record that does not replay (a member it cannot give) opens
/// nothing; and while an opening is held so, a held instrument whose thesis ended is still sold
/// (rule 13).
#[test]
fn an_unreadable_thesis_record_opens_nothing_and_never_holds_an_exit() {
    let mut rows = research_journaled(REGULAR);
    rows.push(
        AGENT_STREAM,
        "ThesisRevised",
        r#"{"thesis_id":"01J8Z3MTHE5000000000000003"}"#,
    );
    assert!(
        research_plan(&rows)
            .plan(&view(), &working_inputs(REGULAR))
            .is_none(),
        "the replay stops at a record it cannot read"
    );
    let held = research_plan(&holding(rows, "3", REGULAR));
    let ending = SignalInputs {
        ended_theses: [InstrumentId::new(SPY).unwrap()].into(),
        ..working_inputs(REGULAR)
    };
    let exit = held
        .plan(&view(), &ending)
        .expect("the ended thesis's holding is sold");
    assert_eq!(exit.side, Side::Sell);
    assert_eq!(exit.purpose, Purpose::DiscretionaryExit);
    assert_eq!(held.classify(&view(), &exit).autonomy, Autonomy::Auto);
}

/// Mandate spec §2, `universe.asset_classes`: an instrument whose admitted thesis names an asset
/// class the envelope does not allow (the research envelope allows `us_equity` alone) is never
/// opened, and a proposal on an instrument the working universe does not name is denied.
#[test]
fn an_instrument_outside_the_envelopes_asset_classes_is_never_opened() {
    let mut crypto = research_registered(REGULAR);
    thesis(
        &mut crypto,
        "01J8Z3MTHE5000000000000001",
        SPY,
        "crypto",
        CURRENT,
    );
    let inputs = SignalInputs {
        thesis_classes: [(
            InstrumentId::new(SPY).unwrap(),
            mandate_accounting::AssetClass::Crypto,
        )]
        .into(),
        ..working_inputs(REGULAR)
    };
    assert!(
        research_plan(&crypto).plan(&view(), &inputs).is_none(),
        "crypto is not allowed"
    );
    let rows = research_journaled(REGULAR);
    let plan = research_plan(&rows);
    let spy = plan
        .plan(&view(), &working_inputs(REGULAR))
        .expect("the allowed opening");
    let unnamed = Proposal {
        instrument: InstrumentId::new(QQQ).unwrap(),
        ..spy
    };
    assert_eq!(plan.classify(&view(), &unnamed).autonomy, Autonomy::Deny);
}

/// `AGENTS.md` rule 3: an opening whose thesis ended between the plan and its classification is
/// denied, since its instrument is no longer the working one.
#[test]
fn an_opening_whose_thesis_ended_before_it_is_classified_is_denied() {
    let mut rows = research_journaled(REGULAR);
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    let plan = Plan::new(
        facts.clone(),
        Mandate::parse(&research_document()).unwrap(),
        FRESH,
    );
    let proposal = plan
        .plan(&view(), &working_inputs(REGULAR))
        .expect("the opening");
    invalidated(&mut rows, "01J8Z3MTHE5000000000000001", SPY);
    facts.feed(&rows.0);
    assert_eq!(plan.classify(&view(), &proposal).autonomy, Autonomy::Deny);
}

/// DEC-2841 item 5: the class a held research instrument is flattened and classified with is the
/// one its own latest admitted thesis record states, never a later record on another instrument,
/// nor a refused one, nor an invalidation.
#[test]
fn a_thesis_class_is_the_latest_admitted_record_of_that_instrument() {
    let mut rows = research_journaled(REGULAR);
    thesis(
        &mut rows,
        "01J8Z3MTHE5000000000000002",
        QQQ,
        "crypto",
        CURRENT,
    );
    let hash = Digest::of(b"x").to_hex();
    rows.push(
        AGENT_STREAM,
        "ThesisProposed",
        &format!(
            r#"{{"model_id":"llm.research_agent","model_version":"0.1.0","content_hash":"sha256:{RESEARCH_HASH}","thesis_id":"01J8Z3MTHE5000000000000003","lineage_id":"01J8Z3MTHE5000000000000003","revision":0,"predecessor_thesis_id":null,"autopsy_ref":null,"instrument_id":"{SPY}","asset_class":"crypto","direction":"long","as_of":"{}","expires_at":"{}","horizon_s":86400,"conviction":"0.6","confidence":"0.6","evidence_ref":null,"evidence_sources":["src.newswire"],"corroboration":"independent_source","invalidation":"a guidance cut","allowlist_version":3,"prompt_ref":"sha256:{hash}","response_ref":"sha256:{hash}","admitted":false,"reason":"corroboration_missing"}}"#,
            CURRENT.0, CURRENT.1
        ),
    );
    invalidated(&mut rows, "01J8Z3MTHE5000000000000001", SPY);
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    assert_eq!(
        facts.thesis_class(SPY),
        Some(mandate_accounting::AssetClass::UsEquity),
        "SPY's own admitted record, not QQQ's, the refused one, or the invalidation"
    );
    assert_eq!(
        facts.thesis_class(QQQ),
        Some(mandate_accounting::AssetClass::Crypto)
    );
    assert_eq!(
        facts.ever_admitted(),
        BTreeMap::from([
            (SPY.to_owned(), mandate_accounting::AssetClass::UsEquity),
            (QQQ.to_owned(), mandate_accounting::AssetClass::Crypto),
        ])
    );
}

/// DEC-2841 item 2: a proposal whose class differs from its thesis record's is denied, even when
/// the proposal's own class is one the envelope allows.
#[test]
fn an_opening_whose_class_differs_from_its_thesis_is_denied() {
    let equity = research_journaled(REGULAR);
    let proposal = research_plan(&equity)
        .plan(&view(), &working_inputs(REGULAR))
        .expect("the opening on SPY as a US equity");
    let mut crypto = research_registered(REGULAR);
    thesis(
        &mut crypto,
        "01J8Z3MTHE5000000000000001",
        SPY,
        "crypto",
        CURRENT,
    );
    assert_eq!(
        research_plan(&crypto).classify(&view(), &proposal).autonomy,
        Autonomy::Deny,
        "its thesis names SPY crypto"
    );
}

/// An owner's request for `qty` SPY, as the runtime hands it to the plan (DEC-2858).
fn request(qty: &str, requested_by: Requester) -> OwnerRequest {
    OwnerRequest {
        request: EventId("01J8Z3MOWNERREQUEST0000001".into()),
        instrument: InstrumentId::new(SPY).unwrap(),
        qty: Qty::parse(qty).unwrap(),
        requested_by,
    }
}

fn owners(qty: &str) -> OwnerRequest {
    request(qty, Requester::Owner)
}

/// The demo mandate with its order cap at `cap` dollars.
fn order_capped(cap: &str) -> Value {
    let mut document: serde_json::Value =
        serde_json::from_slice(&mandate_canon::to_canonical(&document())).unwrap();
    document["risk"]["max_order_usd"] = serde_json::json!(cap);
    mandate_canon::parse(&serde_json::to_vec(&document).unwrap()).unwrap()
}

fn plan_with(rows: &Rows, document: &Value) -> Plan {
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    Plan::new(facts, Mandate::parse(document).unwrap(), FRESH)
}

/// Five SPY at the 99.95 bid, which the plan takes as the ask, is $499.75, under the demo's $1000
/// caps: the owner's five, an opening with the mandate's protection, and no conviction, score,
/// output, weight or clip (journal spec §9.1 rule 8, DEC-2858 item 1).
#[test]
fn an_owners_request_within_every_cap_is_proposed_unclipped_and_unscored() {
    let rows = journaled(REGULAR);
    let proposal = plan_over(&rows)
        .requested(&view(), &owners("5"))
        .expect("the owner's buy");
    assert_eq!(proposal.instrument.as_str(), SPY);
    assert_eq!(proposal.side, Side::Buy);
    assert_eq!(proposal.purpose, Purpose::Open);
    assert_eq!(proposal.qty, Qty::parse("5").unwrap());
    assert_eq!(proposal.limit, Price::parse("99.95").unwrap());
    assert_eq!(proposal.exit_origin, None);
    assert_eq!(proposal.exit_conviction, None);
    assert_eq!(proposal.buy_conviction, None);
    assert_eq!(proposal.combined_score, Value::Null);
    assert!(proposal.outputs_used.is_empty(), "{proposal:?}");
    assert!(proposal.model_weights.is_empty(), "{proposal:?}");
    assert!(proposal.clips_applied.is_empty(), "{proposal:?}");
    let execution = proposal.execution.unwrap();
    assert!(execution.protection_required);
    assert_eq!(
        execution.protection.unwrap().stop,
        Price::parse("94.96").unwrap()
    );
}

/// Twenty is $1999: past the $1000 order cap, the $1000 position cap and the $1000 gross room, so
/// ten shares ($999.50) and all three recorded. Under a $300 order cap, five ($499.75) is cut to
/// three by the order cap alone.
#[test]
fn each_cap_that_cut_the_owners_quantity_is_recorded() {
    let rows = journaled(REGULAR);
    let every = plan_over(&rows)
        .requested(&view(), &owners("20"))
        .expect("the owner's buy, clipped");
    assert_eq!(every.qty, Qty::parse("10").unwrap());
    assert_eq!(
        every.clips_applied,
        vec![
            DecisionClip::MaxOrderUsd,
            DecisionClip::PositionCap,
            DecisionClip::GrossExposureCap,
        ]
    );
    let capped = plan_with(&rows, &order_capped("300"))
        .requested(&view(), &owners("5"))
        .expect("the owner's buy, clipped");
    assert_eq!(capped.qty, Qty::parse("3").unwrap());
    assert_eq!(capped.clips_applied, vec![DecisionClip::MaxOrderUsd]);
}

/// Under a $50 order cap not one share fits: the owner's five go to the gate unclipped, with no
/// clip recorded, for the gate's own reason to name the cap (DEC-2858 item 7).
#[test]
fn a_cap_with_no_room_for_a_share_leaves_the_request_unclipped() {
    let rows = journaled(REGULAR);
    let proposal = plan_with(&rows, &order_capped("50"))
        .requested(&view(), &owners("5"))
        .expect("the owner's buy, unclipped");
    assert_eq!(proposal.qty, Qty::parse("5").unwrap());
    assert!(proposal.clips_applied.is_empty(), "{proposal:?}");
}

/// A request is classified on its own facts: the demo, with no rules, asks by its default, and a
/// client's request is classified the same, the ceiling being the runtime's (§6.2 step 5a).
#[test]
fn the_demo_mandate_asks_the_owner_about_a_request() {
    let rows = journaled(REGULAR);
    let plan = plan_over(&rows);
    for requested_by in [
        Requester::Owner,
        Requester::Client {
            client_id: "client-1".into(),
        },
    ] {
        let proposal = plan
            .requested(&view(), &request("5", requested_by.clone()))
            .expect("the request's buy");
        assert_eq!(
            plan.classify(&view(), &proposal),
            Classified {
                autonomy: Autonomy::Ask,
                decided_by: Some("default".into()),
            },
            "{requested_by:?}"
        );
    }
}

/// A rule that reads `combined_score` cannot be evaluated for a request and denies it by its own
/// id (DEC-2858 item 8), even with `default: auto`; a rule on a fact the plan cannot see still
/// denies under the policy overlay.
#[test]
fn a_rule_reading_the_score_denies_a_request_by_its_own_id() {
    let rows = journaled(REGULAR);
    let mut document: serde_json::Value =
        serde_json::from_slice(&mandate_canon::to_canonical(&document())).unwrap();
    document["autonomy"]["default"] = serde_json::json!("auto");
    document["autonomy"]["rules"] = serde_json::json!([{
        "id": "confident", "then": "auto",
        "when": {"field": "combined_score", "op": "gte", "value": "0.5"},
    }]);
    let scored = mandate_canon::parse(&serde_json::to_vec(&document).unwrap()).unwrap();
    let plan = plan_with(&rows, &scored);
    let proposal = plan
        .requested(&view(), &owners("5"))
        .expect("the owner's buy");
    assert_eq!(
        plan.classify(&view(), &proposal),
        Classified {
            autonomy: Autonomy::Deny,
            decided_by: Some("rule:confident".into()),
        }
    );
    let unseen = plan_with(
        &rows,
        &with_rule(serde_json::json!({"id": "cap", "then": "deny", "when":
            {"field": "drawdown", "op": "gt", "value": "0.05"}})),
    );
    let proposal = unseen
        .requested(&view(), &owners("5"))
        .expect("the owner's buy");
    assert_eq!(
        unseen.classify(&view(), &proposal),
        Classified {
            autonomy: Autonomy::Deny,
            decided_by: Some("policy_overlay".into()),
        }
    );
}

/// The plan proposes no request it could not have sized an opening on: an instrument outside the
/// pins, an agent that holds or has an order working, stale facts, a closed session, a zero
/// quantity, or an unpinned mandate, whose single instrument is its thesis's and not the
/// deployment's own asset (DEC-2858; `AGENTS.md` rule 3).
#[test]
fn a_request_the_plan_could_not_have_opened_on_proposes_nothing() {
    let rows = journaled(REGULAR);
    let mut elsewhere = owners("5");
    elsewhere.instrument = InstrumentId::new("11111111-2222-4333-8444-555555555555").unwrap();
    assert!(plan_over(&rows).requested(&view(), &elsewhere).is_none());
    assert!(plan_over(&rows).requested(&view(), &owners("0")).is_none());
    let held = holding(journaled(REGULAR), "3", REGULAR);
    assert!(
        plan_over(&held).requested(&view(), &owners("5")).is_none(),
        "held"
    );
    assert!(
        plan_over(&journaled(CLOSING))
            .requested(&view(), &owners("5"))
            .is_none(),
        "close window"
    );
    assert!(
        plan_over(&journaled(PRE_MARKET))
            .requested(&view(), &owners("5"))
            .is_none(),
        "pre-market"
    );
    assert!(
        research_plan(&rows)
            .requested(&view(), &owners("5"))
            .is_none(),
        "unpinned"
    );
}
