//! The deployment's gate dry run over the journaled fold (DEC-2388), on the cell's demo mandate:
//! the gate's own checks run over the journaled facts with DEC-2378 item 11's four checks deferred,
//! every opening is still denied while any is deferred, and an exit is never denied.

use std::collections::BTreeSet;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Digest, Value};
use mandate_daemon::production::{Facts, Gate, UNJOURNALED, verdict};
use mandate_gate_context::{Freshness, gate_rule_set};
use mandate_journal::StoredEvent;
use mandate_num::{Price, Qty};
use mandate_risk::{
    Advisory, CheckOutcome, Computed, Decision, Deferred, GateConfig, Purpose as GatePurpose,
    ReasonCode, Verdict,
};
use mandate_runtime::{DryRunVerdict, GateDryRun, Proposal, Purpose};
use mandate_spec::document::Mandate;
use proptest::prelude::*;

const AGENT: &str = "demo-spy";
const CLOCK: &str = "clock:ws1";
const CONTROL: &str = "ctl:ws1";
const ACCOUNT: &str = "acct:ws1:01J8Z3MACCOUNTREF000000001";
const SPY: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const HASH: &str = "cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d";
const FRESH: Freshness = Freshness {
    mark_max_age_s: 60,
    snapshot_max_age_s: 3600,
};
/// Monday 2026-10-26 15:00 UTC, 11:00 in New York: the regular session.
const REGULAR: &str = "2026-10-26T15:00:00.000000000Z";
const DEPLOYED: &str = concat!(
    "sha256:",
    "cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d"
);
const RULES: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/rule-set.json");

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

/// The account a row of [`journaled_with`] snapshots.
#[derive(Debug, Clone)]
struct Account {
    status: &'static str,
    trading_blocked: bool,
    equity: u32,
    cash: u32,
    buying_power: u32,
    non_marginable: u32,
}

const RICH: Account = Account {
    status: "ACTIVE",
    trading_blocked: false,
    equity: 100_000,
    cash: 100_000,
    buying_power: 100_000,
    non_marginable: 100_000,
};

fn journaled_with(account: &Account) -> Rows {
    let mut rows = Rows(Vec::new());
    rows.push(
        CONTROL,
        "AgentDeployed",
        &format!(r#"{{"agent_id":"{AGENT}","mandate_version":"sha256:{HASH}"}}"#),
    );
    rows.push(
        CLOCK,
        "ClockAdvanced",
        &format!(r#"{{"clock":"{REGULAR}"}}"#),
    );
    let Account {
        status,
        trading_blocked,
        equity,
        cash,
        buying_power,
        non_marginable,
    } = account;
    rows.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"{status}","crypto_status":"INACTIVE","trading_blocked":{trading_blocked},"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"{equity}","cash":"{cash}","buying_power":"{buying_power}","non_marginable_buying_power":"{non_marginable}","risk_clock":"{REGULAR}"}}"#
        ),
    );
    rows.push(
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{SPY}","price":"100.05","source":"quote","feed":"fixture","bid":"99.95","sane":true,"session":"regular","risk_clock":"{REGULAR}"}}"#
        ),
    );
    rows
}

fn document() -> Value {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/cell/demo/mandate.json"
    ))
    .unwrap();
    mandate_canon::parse(&bytes).unwrap()
}

/// The deployed version the demo rows name, and its stored document.
fn version() -> (&'static str, Mandate) {
    (DEPLOYED, Mandate::parse(&document()).unwrap())
}

fn config() -> GateConfig {
    gate_rule_set(&mandate_canon::parse(RULES.trim().as_bytes()).unwrap()).unwrap()
}

fn gate_over(rows: &Rows, config: Option<GateConfig>) -> Gate {
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&rows.0);
    Gate::new(facts, version(), config, FRESH)
}

/// An opening buy of `qty` SPY at `limit`, as the plan proposes one.
fn opening(qty: &str, limit: &str) -> Proposal {
    Proposal {
        instrument: InstrumentId::new(SPY).unwrap(),
        asset_class: AssetClass::UsEquity,
        side: Side::Buy,
        qty: Qty::parse(qty).unwrap(),
        limit: Price::parse(limit).unwrap(),
        purpose: Purpose::Open,
        exit_origin: None,
        exit_conviction: Some(Value::Str("0".to_owned())),
        buy_conviction: Some(Value::Str("1".to_owned())),
        combined_score: Value::Str("1".to_owned()),
        outputs_used: BTreeSet::new(),
        model_weights: Default::default(),
        clips_applied: Vec::new(),
        execution: None,
    }
}

fn denied(reason: &str) -> DryRunVerdict {
    DryRunVerdict::Deny {
        reason_code: reason.to_owned(),
    }
}

/// The plan's own opening on the demo: three shares at the bid, $299.85, under every cap.
#[test]
fn an_opening_the_gate_allows_is_still_denied_while_its_checks_are_deferred() {
    let gate = gate_over(&journaled_with(&RICH), Some(config()));
    let proposal = opening("3", "99.95");
    let advisory = gate.advisory(&proposal).unwrap();
    assert_eq!(advisory.decision.verdict, Verdict::Allow, "{advisory:?}");
    assert_eq!(advisory.deferred, BTreeSet::from(UNJOURNALED));
    for deferred in UNJOURNALED {
        assert!(
            advisory
                .decision
                .checks
                .contains(&CheckOutcome::Deferred(deferred.check())),
            "{deferred:?} is reported deferred, never passed: {advisory:?}"
        );
    }
    assert_eq!(gate.check(&proposal), denied("deferred_to_binding"));
}

#[test]
fn every_check_of_dec_2378_item_11_is_deferred_until_its_fact_is_journaled() {
    assert_eq!(
        BTreeSet::from(UNJOURNALED),
        BTreeSet::from([
            Deferred::Eligibility,
            Deferred::LiquidityFloor,
            Deferred::Participation,
            Deferred::DayTradeBudget,
        ])
    );
}

/// The verdict of an advisory answer: only an allow with nothing deferred is an allow.
#[test]
fn only_an_allow_with_nothing_deferred_is_an_allow() {
    let decision = |verdict, reason| Decision {
        verdict,
        reason,
        purpose: GatePurpose::Open,
        pacing: None,
        checks: Vec::new(),
        computed: Computed::default(),
    };
    let advisory = |verdict, reason, deferred: &[Deferred]| Advisory {
        decision: decision(verdict, reason),
        deferred: deferred.iter().copied().collect(),
    };
    assert_eq!(
        verdict(Ok(advisory(Verdict::Allow, None, &[]))),
        DryRunVerdict::Allow
    );
    for deferred in UNJOURNALED {
        assert_eq!(
            verdict(Ok(advisory(Verdict::Allow, None, &[deferred]))),
            denied("deferred_to_binding"),
            "{deferred:?}"
        );
    }
    for gate_verdict in [Verdict::Deny, Verdict::Hold, Verdict::Defer] {
        assert_eq!(
            verdict(Ok(advisory(
                gate_verdict,
                Some(ReasonCode::MaxOrderSize),
                &[]
            ))),
            denied("max_order_size"),
            "{gate_verdict:?}"
        );
        assert_eq!(
            verdict(Ok(advisory(gate_verdict, None, &UNJOURNALED))),
            denied("gate_refused"),
            "{gate_verdict:?} with no reason"
        );
    }
    assert_eq!(verdict(Err("facts_held")), denied("facts_held"));
}

#[test]
fn the_gates_own_reason_is_journaled_before_any_deferral() {
    let gate = gate_over(&journaled_with(&RICH), Some(config()));
    assert_eq!(
        gate.check(&opening("4", "99.95")),
        denied("max_order_size"),
        "$399.80 over the $300 order cap"
    );
    assert_eq!(
        gate.check(&opening("11", "99.95")),
        denied("concentration_limit"),
        "$1099.45 over the $1000 position cap"
    );
    assert_eq!(
        gate.check(&opening("2", "110")),
        denied("price_outside_collar"),
        "a buy limit above the bid's 1% collar"
    );
    let blocked = Account {
        trading_blocked: true,
        ..RICH
    };
    assert_eq!(
        gate_over(&journaled_with(&blocked), Some(config())).check(&opening("3", "99.95")),
        denied("account_trading_blocked")
    );
    let poor = Account { cash: 200, ..RICH };
    assert_eq!(
        gate_over(&journaled_with(&poor), Some(config())).check(&opening("3", "99.95")),
        denied("insufficient_settled_buying_power"),
        "the 1x figure is the lesser of cash and the non-marginable figure"
    );
}

#[test]
fn no_registered_rule_set_denies_every_opening() {
    let gate = gate_over(&journaled_with(&RICH), None);
    assert_eq!(
        gate.advisory(&opening("3", "99.95")),
        Err("rule_set_unavailable")
    );
    assert_eq!(
        gate.check(&opening("3", "99.95")),
        denied("rule_set_unavailable")
    );
}

#[test]
fn a_missing_fact_denies_the_opening() {
    let full = journaled_with(&RICH);
    for dropped in [
        "MarkUpdated",
        "AccountSnapshotRecorded",
        "ClockAdvanced",
        "AgentDeployed",
    ] {
        let rows = Rows(
            full.0
                .iter()
                .filter(|r| r.event_type != dropped)
                .cloned()
                .collect(),
        );
        assert_eq!(
            gate_over(&rows, Some(config())).check(&opening("3", "99.95")),
            denied("facts_held"),
            "without {dropped}"
        );
    }
    let unfollowed = Facts::new(CLOCK, AGENT);
    unfollowed.feed(&full.0);
    let gate = Gate::new(unfollowed, version(), Some(config()), FRESH);
    assert_eq!(gate.check(&opening("3", "99.95")), denied("facts_held"));
}

#[test]
fn an_order_of_the_agents_still_working_denies_the_opening() {
    let mut rows = journaled_with(&RICH);
    rows.push(
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{REGULAR}"}}"#),
    );
    rows.push(
        ACCOUNT,
        "OrderSubmitted",
        r#"{"client_order_id":"o1","instrument_id":"11111111-2222-4333-8444-555555555555","side":"buy"}"#,
    );
    assert_eq!(
        gate_over(&rows, Some(config())).check(&opening("3", "99.95")),
        denied("working_orders_unseen")
    );
    rows.push(
        ACCOUNT,
        "OrderStateChanged",
        r#"{"client_order_id":"o1","state":"canceled","replaced_by":null,"ignored":false}"#,
    );
    assert_eq!(
        gate_over(&rows, Some(config())).check(&opening("3", "99.95")),
        denied("deferred_to_binding"),
        "an order no longer working counts for nothing"
    );
}

/// An exit never consults the fold, the rule set or the gate here: the binding gate judges it
/// (DEC-2378 item 5, `AGENTS.md` rule 13).
#[test]
fn an_exit_is_never_denied() {
    let empty = Facts::new(CLOCK, AGENT);
    let gate = Gate::new(empty, version(), None, FRESH);
    for purpose in [
        Purpose::DiscretionaryExit,
        Purpose::RiskExit,
        Purpose::OwnerExit,
    ] {
        let mut exit = opening("3", "99.95");
        exit.side = Side::Sell;
        exit.purpose = purpose;
        assert_eq!(gate.check(&exit), DryRunVerdict::Allow, "{purpose:?}");
    }
    assert_eq!(
        gate.check(&opening("3", "99.95")),
        denied("rule_set_unavailable"),
        "the same gate denies an opening"
    );
}

/// The verdict computed independently from the trading spec's checks, in §9.1's order, for the
/// demo mandate ($1000 allocation and position cap, $300 order cap), the reviewed rule set (a 1%
/// collar above the ask and a 20% band below it, both rounded inward to the cent) and a bid of
/// 99.95 taken as the ask.
fn oracle(blocked: bool, cash: u32, units: u32, limit_cents: u32) -> DryRunVerdict {
    let cost_cents = u64::from(units) * u64::from(limit_cents);
    let bid_cents = 9_995_u64;
    let floor_cents = (bid_cents * 80).div_ceil(100);
    let ceiling_cents = bid_cents * 101 / 100;
    let limit = u64::from(limit_cents);
    if blocked {
        denied("account_trading_blocked")
    } else if cost_cents > 100_000 {
        denied("concentration_limit")
    } else if cost_cents > 30_000 {
        denied("max_order_size")
    } else if limit < floor_cents || limit > ceiling_cents {
        denied("price_outside_collar")
    } else if cost_cents > u64::from(cash) * 100 {
        denied("insufficient_settled_buying_power")
    } else {
        denied("deferred_to_binding")
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over blocked and active accounts, any cash, any whole quantity and any cent limit, the dry
    /// run answers as the spec's checks computed on their own, and never allows an opening.
    #[test]
    fn the_dry_run_answers_as_the_checks_and_never_allows_an_opening(
        blocked in any::<bool>(),
        cash in 0_u32..2_000,
        units in 1_u32..15,
        limit_cents in 7_000_u32..11_000,
    ) {
        let account = Account { trading_blocked: blocked, cash, ..RICH };
        let gate = gate_over(&journaled_with(&account), Some(config()));
        let cents = format!("{}.{:02}", limit_cents / 100, limit_cents % 100);
        let limit = cents.trim_end_matches('0').trim_end_matches('.');
        let answer = gate.check(&opening(&units.to_string(), limit));
        prop_assert_ne!(&answer, &DryRunVerdict::Allow);
        prop_assert_eq!(answer, oracle(blocked, cash, units, limit_cents));
    }
}

/// One order of the agent's in SPY, journaled as the executor journals it: requested for
/// `purpose`, submitted on `side`, and either filled for `fill`'s quantity at its risk clock or
/// canceled.
fn order(rows: &mut Rows, id: &str, purpose: &str, side: &str, fill: Option<(&str, &str)>) {
    rows.push(
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"{purpose}","risk_clock":"{REGULAR}"}}"#),
    );
    rows.push(
        ACCOUNT,
        "OrderSubmitted",
        &format!(r#"{{"client_order_id":"{id}","instrument_id":"{SPY}","side":"{side}"}}"#),
    );
    let state = match fill {
        Some((qty, filled_at)) => {
            rows.push(
                ACCOUNT,
                "FillApplied",
                &format!(
                    r#"{{"fill_id":"f-{id}","client_order_id":"{id}","instrument_id":"{SPY}","side":"{side}","qty_gross":"{qty}","price":"99.95","risk_clock":"{filled_at}"}}"#
                ),
            );
            "filled"
        }
        None => "canceled",
    };
    rows.push(
        ACCOUNT,
        "OrderStateChanged",
        &format!(
            r#"{{"client_order_id":"{id}","state":"{state}","replaced_by":null,"ignored":false}}"#
        ),
    );
}

/// DEC-2388 item 7: for each fact the fold does not carry, a journal holding a value that should
/// refuse the plan's own opening is never answered an allow with nothing deferred.
#[test]
fn no_unseen_fact_that_should_refuse_is_answered_an_allow_with_nothing_deferred() {
    let thirty_seconds_ago = "2026-10-26T14:59:30.000000000Z";
    let mut exited = journaled_with(&RICH);
    order(
        &mut exited,
        "o1",
        "open",
        "buy",
        Some(("1", thirty_seconds_ago)),
    );
    order(
        &mut exited,
        "o2",
        "discretionary_exit",
        "sell",
        Some(("1", thirty_seconds_ago)),
    );
    let mut busy = journaled_with(&RICH);
    for n in 0..50 {
        order(&mut busy, &format!("c{n}"), "open", "buy", None);
    }
    let mut related = journaled_with(&RICH);
    related.push(
        "acct:ws1:01J8Z3MACCOUNTREF000000002",
        "OrderSubmitted",
        &format!(r#"{{"client_order_id":"r1","instrument_id":"{SPY}","side":"sell"}}"#),
    );
    let mut cash_without_fees = journaled_with(&RICH);
    cash_without_fees.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"299.85","cash":"299.85","buying_power":"299.85","non_marginable_buying_power":"299.85","risk_clock":"{REGULAR}"}}"#
        ),
    );
    for (fact, rows) in [
        (
            "the last exit fill, inside the re-entry cooldown and the opposite-fill interval",
            exited,
        ),
        (
            "50 orders today, at the cap and with no fill against them",
            busy,
        ),
        (
            "a related account's sell resting in the instrument",
            related,
        ),
        (
            "cash for the order's notional and nothing for its fees",
            cash_without_fees,
        ),
    ] {
        let gate = gate_over(&rows, Some(config()));
        let proposal = opening("3", "99.95");
        let advisory = gate.advisory(&proposal);
        assert!(
            !matches!(&advisory, Ok(a) if a.decision.verdict == Verdict::Allow && a.deferred.is_empty()),
            "{fact}: {advisory:?}"
        );
        assert_ne!(gate.check(&proposal), DryRunVerdict::Allow, "{fact}");
    }
}

/// R2's finding on the first review: a proposal labelled as adding risk that the gate types as an
/// exit would defer nothing, so the dry run refuses to judge it rather than allow it.
#[test]
fn a_proposal_labelled_an_opening_that_the_gate_types_as_an_exit_is_denied() {
    let mut rows = journaled_with(&RICH);
    order(&mut rows, "o1", "open", "buy", Some(("3", REGULAR)));
    let gate = gate_over(&rows, Some(config()));
    for purpose in [Purpose::Open, Purpose::Increase] {
        let mut sell = opening("3", "99.95");
        sell.side = Side::Sell;
        sell.purpose = purpose;
        assert_eq!(gate.advisory(&sell), Err("gate_refused"), "{purpose:?}");
        assert_eq!(gate.check(&sell), denied("gate_refused"), "{purpose:?}");
    }
    let mut increase = opening("1", "99.95");
    increase.purpose = Purpose::Increase;
    assert_eq!(
        gate.check(&increase),
        denied("deferred_to_binding"),
        "a buy while holding is the gate's increase, still deferred"
    );
}

#[test]
fn a_mandate_version_applied_after_the_start_holds_every_opening() {
    let applied = |version: &str| {
        let mut rows = journaled_with(&RICH);
        rows.push(
            ACCOUNT,
            "MandateVersionApplied",
            &format!(
                r#"{{"agent_id":"{AGENT}","new_version":"{version}","result":"applied","risk_clock":"{REGULAR}"}}"#
            ),
        );
        gate_over(&rows, Some(config())).check(&opening("3", "99.95"))
    };
    assert_eq!(
        applied(DEPLOYED),
        denied("deferred_to_binding"),
        "the version the gate was built on"
    );
    assert_eq!(
        applied(&format!("sha256:{}", "0".repeat(64))),
        denied("facts_held"),
        "a version the gate was not built on"
    );
}

#[test]
fn a_held_instrument_with_no_mark_holds_the_opening() {
    let other = "11111111-2222-4333-8444-555555555555";
    let mut rows = journaled_with(&RICH);
    rows.push(
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{REGULAR}"}}"#),
    );
    rows.push(
        ACCOUNT,
        "OrderSubmitted",
        &format!(r#"{{"client_order_id":"q1","instrument_id":"{other}","side":"buy"}}"#),
    );
    rows.push(
        ACCOUNT,
        "FillApplied",
        &format!(
            r#"{{"fill_id":"fq1","client_order_id":"q1","instrument_id":"{other}","side":"buy","qty_gross":"1","price":"50","risk_clock":"{REGULAR}"}}"#
        ),
    );
    rows.push(
        ACCOUNT,
        "OrderStateChanged",
        r#"{"client_order_id":"q1","state":"filled","replaced_by":null,"ignored":false}"#,
    );
    assert_eq!(
        gate_over(&rows, Some(config())).check(&opening("3", "99.95")),
        denied("facts_held"),
        "its market value cannot be counted against the caps"
    );
}
