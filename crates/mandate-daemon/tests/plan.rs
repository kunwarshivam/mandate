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
    ApprovalSettings, Autonomy, Classified, MandateView, ModelDirection, ModelOutput, OrderPlan,
    Proposal, Purpose, RiskClock, SignalInputs,
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
