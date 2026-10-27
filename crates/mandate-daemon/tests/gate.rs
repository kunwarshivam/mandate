//! The deployment's gate dry run over the journaled fold (DEC-2388), on the cell's demo mandate:
//! the gate's own checks run over the journaled facts with DEC-2378 item 11's four checks deferred,
//! an opening they allow is answered with every deferral named, so the runtime asks the owner and
//! none is reported passed (DEC-2388 item 13), and an exit is never denied.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_bars::LiquidityFacts;
use mandate_canon::{DecStr, Digest, Value};
use mandate_daemon::bars::BarsReader;
use mandate_daemon::production::{Facts, Gate, Liquidity, UNJOURNALED, UNSEEN, deferral, verdict};
use mandate_gate_context::{Freshness, gate_rule_set};
use mandate_journal::StoredEvent;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{
    AssetClass as MarketAssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol,
};
use mandate_num::{Fraction, Price, Qty, Usd};
use mandate_risk::{
    Advisory, CheckOutcome, Computed, Decision, Deferred, GateConfig, Purpose as GatePurpose,
    ReasonCode, Verdict,
};
use mandate_runtime::{Deferral, DryRunVerdict, GateDryRun, LiquidityRead, Proposal, Purpose};
use mandate_spec::document::Mandate;
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_midnight};
use proptest::prelude::*;

const AGENT: &str = "demo-spy";
const CLOCK: &str = "clock:ws1";
const CONTROL: &str = "ctl:ws1";
const ACCOUNT: &str = "acct:ws1:01J8Z3MACCOUNTREF000000001";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
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
        CONTROL,
        "ConnectionEstablished",
        r#"{"connection_id":"conn-1"}"#,
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

/// The cell's demo envelope with the $300 order cap these cases' arithmetic is written for: the
/// demo's own cap is one whole share's room, and these cases test the gate's cap, not the demo's.
fn document() -> Value {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/cell/demo/mandate.json"
    ))
    .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let capped = text.replace(r#""max_order_usd":"1000""#, r#""max_order_usd":"300""#);
    assert_ne!(
        capped, text,
        "the demo envelope's order cap is where these cases set it"
    );
    mandate_canon::parse(capped.as_bytes()).unwrap()
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

/// The eleven parts the runtime names beside the liquidity floor and participation, written out
/// by hand: every one the dry run defers whatever the bars say. The re-entry cooldown is not one:
/// the dry run reads the last exit fill from the fold, as the binding gate does (mandate spec §5.3).
fn eleven() -> BTreeSet<Deferral> {
    BTreeSet::from([
        Deferral::AccountExposure,
        Deferral::AgentEquity,
        Deferral::AgentMode,
        Deferral::ConductHistory,
        Deferral::DayTradeBudget,
        Deferral::Eligibility,
        Deferral::FeeReservation,
        Deferral::Halt,
        Deferral::OtherOrders,
        Deferral::QuoteAsk,
        Deferral::RelatedAccounts,
    ])
}

/// An opening the gate allows with no bars read: all twelve deferred, and both liquidity checks
/// with them.
fn asked() -> DryRunVerdict {
    DryRunVerdict::AllowDeferring {
        deferred: eleven(),
        liquidity: None,
    }
}

/// `facts` as the dry run records them, read at the fold's risk clock.
fn read_of(facts: &LiquidityFacts) -> LiquidityRead {
    LiquidityRead {
        prior_close: facts.prior_close,
        median_dollar_volume_20d: facts.median_dollar_volume_20d,
        adv_20d: facts.adv_20d,
        trailing_5m_volume: facts.trailing_5m_volume,
        read_at: UtcNanos::parse(REGULAR).unwrap(),
    }
}

/// An opening the gate allows on `facts`: the twelve deferred, the liquidity checks evaluated.
fn asked_on(facts: &LiquidityFacts) -> DryRunVerdict {
    DryRunVerdict::AllowDeferring {
        deferred: eleven(),
        liquidity: Some(read_of(facts)),
    }
}

fn denied_on(reason: &str, facts: &LiquidityFacts) -> DryRunVerdict {
    DryRunVerdict::DenyOnFigures {
        reason_code: reason.to_owned(),
        liquidity: read_of(facts),
    }
}

/// The plan's own opening on the demo: three shares at the bid, $299.85, under every cap.
#[test]
fn an_opening_the_gate_allows_is_answered_with_every_deferral_named() {
    let gate = gate_over(&journaled_with(&RICH), Some(config()));
    let proposal = opening("3", "99.95");
    let advisory = gate.advisory(&proposal).unwrap();
    assert_eq!(advisory.decision.verdict, Verdict::Allow, "{advisory:?}");
    let every: BTreeSet<Deferred> = UNJOURNALED.into_iter().chain(UNSEEN).collect();
    assert_eq!(
        every,
        BTreeSet::from([
            Deferred::Eligibility,
            Deferred::LiquidityFloor,
            Deferred::Participation,
            Deferred::DayTradeBudget,
            Deferred::AgentMode,
            Deferred::AgentEquity,
            Deferred::Halt,
            Deferred::OtherOrders,
            Deferred::QuoteAsk,
            Deferred::ConductHistory,
            Deferred::RelatedAccounts,
            Deferred::FeeReservation,
            Deferred::AccountExposure,
        ]),
        "DEC-2378 item 11's four and DEC-2388 item 8's nine, pinned by name: the re-entry cooldown \
         is read from the fold (DEC-2851)"
    );
    assert_eq!(advisory.deferred, every);
    assert!(
        !advisory
            .decision
            .checks
            .iter()
            .any(|outcome| matches!(outcome, CheckOutcome::Passed(_))),
        "nothing the dry run cannot see is reported passed: {advisory:?}"
    );
    for deferred in every {
        assert!(
            advisory
                .decision
                .checks
                .contains(&CheckOutcome::Deferred(deferred.check())),
            "{deferred:?} is reported deferred, never passed: {advisory:?}"
        );
    }
    assert_eq!(gate.check(&proposal), asked());
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

/// The verdict of an advisory answer: an allow with nothing deferred is an allow, an allow with
/// any deferral names each, a refusal carries its reason, and the figures go with each.
#[test]
fn each_advisory_answer_maps_to_its_verdict() {
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
    let figures = read_of(&liquid());
    assert_eq!(
        verdict(Ok((
            advisory(Verdict::Allow, None, &[]),
            Some(figures.clone())
        ))),
        DryRunVerdict::Allow
    );
    assert_eq!(
        verdict(Ok((advisory(Verdict::Allow, None, &[]), None))),
        DryRunVerdict::AllowDeferring {
            deferred: BTreeSet::new(),
            liquidity: None
        },
        "no figures read leaves both liquidity checks to the binding gate"
    );
    for deferred in UNJOURNALED.into_iter().chain(UNSEEN) {
        let liquid = matches!(deferred, Deferred::LiquidityFloor | Deferred::Participation);
        assert_eq!(
            verdict(Ok((
                advisory(Verdict::Allow, None, &[deferred]),
                Some(figures.clone())
            ))),
            DryRunVerdict::AllowDeferring {
                deferred: deferral(deferred).into_iter().collect(),
                liquidity: (!liquid).then(|| figures.clone()),
            },
            "{deferred:?}: a liquidity check still deferred drops the figures"
        );
    }
    for gate_verdict in [Verdict::Deny, Verdict::Hold, Verdict::Defer] {
        assert_eq!(
            verdict(Ok((
                advisory(gate_verdict, Some(ReasonCode::MaxOrderSize), &[]),
                None
            ))),
            denied("max_order_size"),
            "{gate_verdict:?}"
        );
        assert_eq!(
            verdict(Ok((
                advisory(gate_verdict, Some(ReasonCode::MaxOrderSize), &UNSEEN),
                Some(figures.clone())
            ))),
            DryRunVerdict::DenyOnFigures {
                reason_code: "max_order_size".to_owned(),
                liquidity: figures.clone()
            },
            "{gate_verdict:?} on figures"
        );
        assert_eq!(
            verdict(Ok((advisory(gate_verdict, None, &UNJOURNALED), None))),
            denied("gate_refused"),
            "{gate_verdict:?} with no reason"
        );
    }
    assert_eq!(verdict(Err("facts_held")), denied("facts_held"));
}

/// Each part the gate defers is named by the runtime as the journal names it: the liquidity floor
/// and participation by the runtime itself, the other twelve by the part of the same name. The
/// names are read from the journal's vectors, not from either crate.
#[test]
fn each_deferred_part_is_named_as_the_journal_names_it() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/refcases/journal.json"
    );
    let fixture = mandate_canon::parse(&std::fs::read(path).unwrap()).unwrap();
    let parts: BTreeSet<&str> = fixture
        .get("deferred_to_binding")
        .and_then(|section| section.get("parts"))
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let by_hand = [
        (Deferred::Eligibility, "eligibility"),
        (Deferred::LiquidityFloor, "liquidity_floor"),
        (Deferred::Participation, "participation"),
        (Deferred::DayTradeBudget, "day_trade_budget"),
        (Deferred::AgentMode, "agent_mode"),
        (Deferred::AgentEquity, "agent_equity"),
        (Deferred::ReentryCooldown, "reentry_cooldown"),
        (Deferred::Halt, "halt"),
        (Deferred::OtherOrders, "other_orders"),
        (Deferred::QuoteAsk, "quote_ask"),
        (Deferred::ConductHistory, "conduct_history"),
        (Deferred::RelatedAccounts, "related_accounts"),
        (Deferred::FeeReservation, "fee_reservation"),
        (Deferred::AccountExposure, "account_exposure"),
    ];
    assert_eq!(
        by_hand
            .iter()
            .map(|(_, name)| *name)
            .collect::<BTreeSet<_>>(),
        parts
    );
    for (deferred, name) in by_hand {
        match deferral(deferred) {
            Some(named) => assert_eq!(named.name(), name, "{deferred:?}"),
            None => assert!(LIQUIDITY_PARTS.contains(&name), "{deferred:?}"),
        }
    }
    assert_eq!(
        Deferral::ALL.len() + LIQUIDITY_PARTS.len(),
        parts.len(),
        "every runtime name is a journal part"
    );
}

/// The two parts the runtime names exactly when no figures were evaluated (journal spec rule 270).
const LIQUIDITY_PARTS: [&str; 2] = ["liquidity_floor", "participation"];

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
        asked(),
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
        asked()
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over blocked and active accounts, any cash, any whole quantity and any cent limit, the dry
    /// run answers as the spec's checks computed on their own, and never allows an opening with
    /// nothing deferred.
    #[test]
    fn the_dry_run_answers_as_the_checks_and_never_allows_an_opening_unasked(
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
    for (fact, part, rows) in [
        (
            "50 orders today, at the cap and with no fill against them",
            Deferred::ConductHistory,
            busy,
        ),
        (
            "a related account's sell resting in the instrument",
            Deferred::RelatedAccounts,
            related,
        ),
        (
            "cash for the order's notional and nothing for its fees",
            Deferred::FeeReservation,
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
        assert!(
            advisory.is_ok(),
            "{fact}: evaluated, not held: {advisory:?}"
        );
        if let Ok(advisory) = &advisory {
            assert!(advisory.deferred.contains(&part), "{fact}: {advisory:?}");
            assert!(
                !advisory
                    .decision
                    .checks
                    .contains(&CheckOutcome::Passed(part.check())),
                "{fact}: its check is never reported passed: {advisory:?}"
            );
        }
    }
}

/// R2's finding on the first review: a proposal labelled as adding risk that the gate types as an
/// exit would defer nothing, so the dry run never allows it; with the position it would sell held,
/// the account's opening hold refuses it first (DEC-2851).
#[test]
fn a_proposal_labelled_an_opening_that_the_gate_types_as_an_exit_is_denied() {
    let mut rows = journaled_with(&RICH);
    order(&mut rows, "o1", "open", "buy", Some(("3", REGULAR)));
    let gate = gate_over(&rows, Some(config()));
    for purpose in [Purpose::Open, Purpose::Increase] {
        let mut sell = opening("3", "99.95");
        sell.side = Side::Sell;
        sell.purpose = purpose;
        assert_eq!(gate.advisory(&sell), Err("account_traded"), "{purpose:?}");
        assert_eq!(gate.check(&sell), denied("account_traded"), "{purpose:?}");
    }
    let mut increase = opening("1", "99.95");
    increase.purpose = Purpose::Increase;
    assert_eq!(
        gate.check(&increase),
        denied("account_traded"),
        "a buy while holding is held, as the binding gate holds every opening and increase then \
         (DEC-2143, DEC-2851)"
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
        asked(),
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
        denied("account_traded"),
        "a held position holds the opening before its market value is counted, as the binding \
         gate holds it (DEC-2143, DEC-2851)"
    );
}

/// A later snapshot of the demo's rich account with `status`, `crypto_status` and the three
/// blocking flags as given.
fn snapshot(rows: &mut Rows, status: &str, crypto_status: &str, flags: [bool; 3]) {
    let [trading, account, suspended] = flags;
    rows.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"{status}","crypto_status":"{crypto_status}","trading_blocked":{trading},"account_blocked":{account},"trade_suspended_by_user":{suspended},"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","risk_clock":"{REGULAR}"}}"#
        ),
    );
}

/// Trading spec §7.3's first row, as the executor's fold reads it: any blocking flag, or a status
/// other than `ACTIVE`, blocks the account; each alone suffices.
#[test]
fn each_blocking_flag_and_any_status_but_active_blocks_the_account() {
    let check = |status: &str, flags: [bool; 3]| {
        let mut rows = journaled_with(&RICH);
        snapshot(&mut rows, status, "INACTIVE", flags);
        gate_over(&rows, Some(config())).check(&opening("3", "99.95"))
    };
    assert_eq!(check("ACTIVE", [false; 3]), asked());
    for flags in [
        [true, false, false],
        [false, true, false],
        [false, false, true],
    ] {
        assert_eq!(
            check("ACTIVE", flags),
            denied("account_trading_blocked"),
            "{flags:?}"
        );
    }
    for status in ["ACCOUNT_CLOSED", "ONBOARDING", "active"] {
        assert_eq!(
            check(status, [false; 3]),
            denied("account_trading_blocked"),
            "{status}"
        );
    }
}

/// A crypto opening needs the account's crypto status `ACTIVE`, and nothing else reads as active.
#[test]
fn a_crypto_opening_needs_an_active_crypto_account() {
    let check = |crypto_status: &str| {
        let mut rows = journaled_with(&RICH);
        snapshot(&mut rows, "ACTIVE", crypto_status, [false; 3]);
        let mut crypto = opening("3", "99.95");
        crypto.asset_class = AssetClass::Crypto;
        gate_over(&rows, Some(config())).check(&crypto)
    };
    for inactive in ["INACTIVE", "SUBMITTED", "active"] {
        assert_eq!(
            check(inactive),
            denied("crypto_account_inactive"),
            "{inactive}"
        );
    }
    assert_ne!(
        check("ACTIVE"),
        denied("crypto_account_inactive"),
        "an active crypto account passes check 1"
    );
}

/// A liquidity port that answers `facts` to every read and counts the reads.
struct Figures {
    facts: Option<LiquidityFacts>,
    reads: Rc<Cell<u32>>,
    purposes: Rc<RefCell<Vec<GatePurpose>>>,
}

impl Liquidity for Figures {
    fn opening(&self, purpose: GatePurpose, symbol: &str, now: UtcNanos) -> Option<LiquidityFacts> {
        assert_eq!(symbol, "SPY", "the pinned instrument's symbol is read");
        assert_eq!(
            now,
            UtcNanos::parse(REGULAR).unwrap(),
            "read at the fold's risk clock"
        );
        self.reads.set(self.reads.get() + 1);
        self.purposes.borrow_mut().push(purpose);
        self.facts.clone()
    }
}

/// SPY's figures: a close of `close` and a 20-day median dollar volume of `median`, an ADV of
/// `adv` shares and `trailing` shares in the last five minutes.
fn figures(close: &str, median: &str, adv: &str, trailing: &str) -> LiquidityFacts {
    LiquidityFacts {
        prior_close: Price::parse(close).unwrap(),
        median_dollar_volume_20d: Usd::parse(median).unwrap(),
        adv_20d: Qty::parse(adv).unwrap(),
        trailing_5m_volume: Qty::parse(trailing).unwrap(),
    }
}

fn liquid() -> LiquidityFacts {
    figures("99.95", "20000000000", "80000000", "500000")
}

fn gate_reading(rows: &Rows, facts: Option<LiquidityFacts>) -> (Gate, Rc<Cell<u32>>) {
    let (gate, reads, _) = gate_reading_under(rows, facts, config());
    (gate, reads)
}

/// A gate with the count of its reads and the purposes it read for.
type ReadingGate = (Gate, Rc<Cell<u32>>, Rc<RefCell<Vec<GatePurpose>>>);

/// A gate over `rows` under the gate configuration `config`, reading `facts`, with the count of
/// its reads and the purposes it read for.
fn gate_reading_under(
    rows: &Rows,
    facts: Option<LiquidityFacts>,
    config: GateConfig,
) -> ReadingGate {
    let reads = Rc::new(Cell::new(0));
    let purposes = Rc::new(RefCell::new(Vec::new()));
    let fold = Facts::new(CLOCK, AGENT);
    fold.follow(ACCOUNT);
    fold.feed(&rows.0);
    let gate = Gate::new(fold, version(), Some(config), FRESH).reading(Box::new(Figures {
        facts,
        reads: Rc::clone(&reads),
        purposes: Rc::clone(&purposes),
    }));
    (gate, reads, purposes)
}

/// DEC-2274 item 3: with the bars read, the liquidity floor and participation are evaluated,
/// never deferred, and the other eleven stay deferred, so the opening is asked with its figures.
#[test]
fn with_the_bars_read_the_floor_and_participation_are_evaluated() {
    let (gate, reads) = gate_reading(&journaled_with(&RICH), Some(liquid()));
    let advisory = gate.advisory(&opening("3", "99.95")).unwrap();
    assert_eq!(reads.get(), 1);
    assert_eq!(advisory.decision.verdict, Verdict::Allow, "{advisory:?}");
    assert!(!advisory.deferred.contains(&Deferred::LiquidityFloor));
    assert!(!advisory.deferred.contains(&Deferred::Participation));
    assert_eq!(
        advisory.deferred.len(),
        11,
        "eligibility, the day-trade budget and the nine unseen parts: {advisory:?}"
    );
    assert_eq!(gate.check(&opening("3", "99.95")), asked_on(&liquid()));
}

/// DEC-2378 item 11 condition 2: each check moves from deferred to evaluated when its bars appear,
/// with nothing else changed. Without bars a refusing figure is never read, so both stay deferred;
/// with them, each refuses with the binding gate's reason.
#[test]
fn each_liquidity_check_moves_from_deferred_to_evaluated_when_its_bars_appear() {
    for (check, facts, reason) in [
        (
            "the price floor",
            figures("4", "20000000000", "80000000", "500000"),
            "below_price_floor",
        ),
        (
            "the liquidity floor",
            figures("99.95", "999999", "80000000", "500000"),
            "below_liquidity_floor",
        ),
        (
            "the order-size participation",
            figures("99.95", "20000000000", "80000000", "59"),
            "conduct_limit_breached",
        ),
        (
            "the daily participation",
            figures("99.95", "20000000000", "59", "500000"),
            "conduct_limit_breached",
        ),
    ] {
        let rows = journaled_with(&RICH);
        let (without, _) = gate_reading(&rows, None);
        let deferred = without.advisory(&opening("3", "99.95")).unwrap();
        assert!(
            deferred.deferred.contains(&Deferred::LiquidityFloor)
                && deferred.deferred.contains(&Deferred::Participation),
            "{check}: deferred without bars"
        );
        assert_eq!(without.check(&opening("3", "99.95")), asked(), "{check}");
        let (with, _) = gate_reading(&rows, Some(facts.clone()));
        assert_eq!(
            with.check(&opening("3", "99.95")),
            denied_on(reason, &facts),
            "{check}"
        );
    }
}

/// DEC-2274 item 6, the call site's test: an exit never reaches the reader, even with a readable
/// store: neither the dry run nor its advisory pass called directly on an exit label reads it.
#[test]
fn an_exit_never_reaches_the_reader() {
    let (gate, reads) = gate_reading(&journaled_with(&RICH), Some(liquid()));
    for purpose in [
        Purpose::DiscretionaryExit,
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::Protective,
        Purpose::Flatten,
    ] {
        let mut exit = opening("3", "99.95");
        exit.side = Side::Sell;
        exit.purpose = purpose;
        assert_eq!(gate.check(&exit), DryRunVerdict::Allow, "{purpose:?}");
        assert_ne!(
            verdict(gate.read(&exit)),
            DryRunVerdict::Allow,
            "{purpose:?}: the advisory pass called directly on an exit label never allows it"
        );
    }
    assert_eq!(reads.get(), 0, "no exit read the bars");
    gate.check(&opening("3", "99.95"));
    assert_eq!(reads.get(), 1, "an opening does");
}

/// The production reader over a stored root: the same figures `mandate_bars::liquidity` gives, at
/// the fold's risk clock; a root with no dataset for SPY keeps both checks deferred.
#[test]
fn the_bars_reader_reads_the_stored_root_at_the_risk_clock() {
    let root =
        std::env::temp_dir().join(format!("mandate-daemon-gate-bars-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    store_spy(&root);
    let rows = journaled_with(&RICH);
    let fold = Facts::new(CLOCK, AGENT);
    fold.follow(ACCOUNT);
    fold.feed(&rows.0);
    let gate = Gate::new(fold, version(), Some(config()), FRESH)
        .reading(Box::new(BarsReader::new(root.to_str())));
    let advisory = gate.advisory(&opening("3", "99.95")).unwrap();
    assert!(
        !advisory.deferred.contains(&Deferred::LiquidityFloor)
            && !advisory.deferred.contains(&Deferred::Participation),
        "{advisory:?}"
    );
    assert_eq!(
        BarsReader::new(root.to_str()).opening(
            GatePurpose::Open,
            "SPY",
            UtcNanos::parse(REGULAR).unwrap()
        ),
        Some(mandate_bars::liquidity(&root, "SPY", UtcNanos::parse(REGULAR).unwrap()).unwrap()),
        "the production reader's figures are mandate-bars' at the risk clock"
    );
    std::fs::remove_dir_all(&root).ok();
    let empty = gate_over(&rows, Some(config()));
    let none = empty.advisory(&opening("3", "99.95")).unwrap();
    assert!(none.deferred.contains(&Deferred::LiquidityFloor));
    assert!(none.deferred.contains(&Deferred::Participation));
}

/// SPY's last 30 sessions to 2026-10-23, each closing 99.95 on 2,000,000 shares, and its minutes
/// of 2026-10-26 from the open, as the data process stores them.
fn store_spy(root: &std::path::Path) {
    let store = Store::new(root);
    let dataset = |timeframe: &str| {
        DatasetId::new(
            MarketAssetClass::UsEquity,
            Feed::Iex,
            Kind::Bars(timeframe.parse().unwrap()),
            Symbol::parse("SPY").unwrap(),
        )
        .unwrap()
    };
    let bar = |start: UtcNanos, volume: &str| {
        let price = DecStr::parse("99.95").unwrap();
        Bar {
            start,
            open: price.clone(),
            high: price.clone(),
            low: price.clone(),
            close: price.clone(),
            volume: DecStr::parse(volume).unwrap(),
            vwap: price,
            trade_count: 10,
        }
    };
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let mut days = Vec::new();
    let mut day = Date::new(2026, 8, 1).unwrap();
    while day <= Date::new(2026, 10, 23).unwrap() {
        if calendar.is_trading_day(day).unwrap() {
            days.push(day);
        }
        day = day.next().unwrap();
    }
    for day in days.split_off(days.len() - 30) {
        let bars = vec![bar(new_york_midnight(day).unwrap(), "2000000")];
        store
            .put_day(&dataset("1Day"), day, &Records::Bars(bars))
            .unwrap();
    }
    let open = UtcNanos::parse("2026-10-26T13:30:00.000000000Z")
        .unwrap()
        .secs();
    let minutes = (0..390)
        .map(|n| bar(UtcNanos::from_parts(open + 60 * n, 0).unwrap(), "5000"))
        .collect();
    store
        .put_day(
            &dataset("1Min"),
            Date::new(2026, 10, 26).unwrap(),
            &Records::Bars(minutes),
        )
        .unwrap();
}

/// Each participation cap reads its own figure: under an order-size cap of 1% and a daily cap of
/// 5%, a 3-share opening passes on 300 shares in the last five minutes and an ADV of 100, and
/// fails on the figures swapped, so a build that filled one figure into the other's field would
/// answer the swapped case for the true one.
#[test]
fn each_participation_cap_reads_its_own_figure() {
    let mut tight = config();
    tight.order_size_participation = Fraction::parse("0.01").unwrap();
    let rows = journaled_with(&RICH);
    let passing = figures("99.95", "20000000000", "100", "300");
    let (passes, _, _) = gate_reading_under(&rows, Some(passing.clone()), tight.clone());
    assert_eq!(
        passes.check(&opening("3", "99.95")),
        asked_on(&passing),
        "order cap 3 of 300 trailing, daily cap 5 of 100 ADV"
    );
    let swapping = figures("99.95", "20000000000", "300", "100");
    let (swapped, _, _) = gate_reading_under(&rows, Some(swapping.clone()), tight);
    assert_eq!(
        swapped.check(&opening("3", "99.95")),
        denied_on("conduct_limit_breached", &swapping),
        "order cap 1 of 100 trailing"
    );
}

/// The bars are read for the purpose the proposal is labelled with, an opening's or an
/// increase's, once per dry run.
#[test]
fn the_bars_are_read_for_an_opening_or_an_increase_once() {
    for (purpose, read_as) in [
        (Purpose::Open, GatePurpose::Open),
        (Purpose::Increase, GatePurpose::Increase),
    ] {
        let (gate, reads, purposes) =
            gate_reading_under(&journaled_with(&RICH), Some(liquid()), config());
        let mut proposal = opening("3", "99.95");
        proposal.purpose = purpose;
        let advisory = gate.advisory(&proposal).unwrap();
        assert!(
            !advisory.deferred.contains(&Deferred::LiquidityFloor),
            "{purpose:?}"
        );
        assert_eq!(reads.get(), 1, "{purpose:?}");
        assert_eq!(*purposes.borrow(), vec![read_as], "{purpose:?}");
    }
}

/// The stored bars are US equities' alone, so a crypto opening never reads them and both checks
/// stay deferred.
#[test]
fn a_crypto_opening_never_reads_the_equity_bars() {
    let (gate, reads) = gate_reading(&journaled_with(&RICH), Some(liquid()));
    let mut crypto = opening("3", "99.95");
    crypto.asset_class = AssetClass::Crypto;
    let _crypto_is_denied_on_the_inactive_crypto_account = gate.check(&crypto);
    assert_eq!(reads.get(), 0);
}

/// A version 2 snapshot of the rich account, later than [`journaled_with`]'s, declaring
/// `account_type` and `day_trading_regime` (JSON `null` for none) and the prior-close equity
/// (journal spec v0.89 §9.35).
fn declared(rows: &mut Rows, account_type: &str, regime: &str, last_equity: &str) {
    let quoted = |value: &str| {
        if value == "null" {
            value.to_owned()
        } else {
            format!("\"{value}\"")
        }
    };
    rows.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","account_type":{},"day_trading_regime":{},"last_equity":"{last_equity}","maintenance_margin":"0","risk_clock":"{REGULAR}"}}"#,
            quoted(account_type),
            quoted(regime),
        ),
    );
}

/// DEC-2811 (DEC-2378 item 11 condition 2): the day-trade budget moves from deferred to evaluated
/// exactly when a version 2 snapshot declares the facts under which trading spec §9.2's check
/// cannot refuse an opening: an `intraday_margin` regime, a cash account, or `legacy_pdt` with a
/// prior-close equity at or above the rule set's 25000 threshold. The opening is then asked with
/// the other eleven parts deferred, and the budget reported evaluated, never deferred.
#[test]
fn the_day_trade_budget_is_evaluated_once_a_snapshot_declares_it_cannot_refuse() {
    for (account_type, regime, last_equity) in [
        ("margin", "intraday_margin", "100"),
        ("null", "intraday_margin", "100"),
        ("cash", "legacy_pdt", "100"),
        ("cash", "null", "100"),
        ("margin", "legacy_pdt", "25000"),
        ("null", "legacy_pdt", "30000"),
    ] {
        let mut rows = journaled_with(&RICH);
        declared(&mut rows, account_type, regime, last_equity);
        let gate = gate_over(&rows, Some(config()));
        let case = format!("{account_type} {regime} {last_equity}");
        let advisory = gate.advisory(&opening("3", "99.95")).unwrap();
        assert!(
            !advisory.deferred.contains(&Deferred::DayTradeBudget),
            "{case}: {advisory:?}"
        );
        let mut ten = eleven();
        ten.remove(&Deferral::DayTradeBudget);
        assert_eq!(
            gate.check(&opening("3", "99.95")),
            DryRunVerdict::AllowDeferring {
                deferred: ten,
                liquidity: None
            },
            "{case}"
        );
    }
}

/// And it stays deferred, never reported passed, whenever the check could refuse on a fact nothing
/// journals: a version 1 snapshot, an undeclared regime on a margin account, or `legacy_pdt` below
/// the threshold on an account not declared cash, where the pattern-day-trader flag and the
/// trailing day-trade count are unseen (DEC-2398's review).
#[test]
fn the_day_trade_budget_stays_deferred_while_it_could_refuse_unseen() {
    let mut cases: Vec<Rows> = vec![journaled_with(&RICH)];
    for (account_type, regime, last_equity) in [
        ("margin", "null", "100"),
        ("null", "null", "100"),
        ("margin", "legacy_pdt", "24999.99"),
        ("null", "legacy_pdt", "100"),
        ("margin", "legacy_pdt", "-5"),
    ] {
        let mut rows = journaled_with(&RICH);
        declared(&mut rows, account_type, regime, last_equity);
        cases.push(rows);
    }
    for (index, rows) in cases.iter().enumerate() {
        let gate = gate_over(rows, Some(config()));
        let advisory = gate.advisory(&opening("3", "99.95")).unwrap();
        assert!(
            advisory.deferred.contains(&Deferred::DayTradeBudget),
            "case {index}: {advisory:?}"
        );
        assert_eq!(gate.check(&opening("3", "99.95")), asked(), "case {index}");
    }
}

/// A later version 1 snapshot takes the declared facts away again, so the budget is deferred
/// once more: the latest snapshot wins whatever its version (§9.35).
#[test]
fn a_later_version_1_snapshot_defers_the_budget_again() {
    let mut rows = journaled_with(&RICH);
    declared(&mut rows, "margin", "intraday_margin", "100");
    snapshot(&mut rows, "ACTIVE", "INACTIVE", [false; 3]);
    let gate = gate_over(&rows, Some(config()));
    assert_eq!(gate.check(&opening("3", "99.95")), asked());
}

/// The context's account type is the snapshot's declared one, else the multiplier's (DEC-2811):
/// a cash account's buying-power refusal is the settled one, a margin account's is not, so the type
/// the dry run reads is the one the reason names.
#[test]
fn the_buying_power_refusal_names_the_declared_account_type_else_the_multipliers() {
    let poor = |multiplier: u32, declared: &str| {
        let mut rows = journaled_with(&RICH);
        let members = if declared == "none" {
            String::new()
        } else {
            format!(
                r#","account_type":{declared},"day_trading_regime":null,"last_equity":"100","maintenance_margin":"0""#
            )
        };
        rows.push(
            ACCOUNT,
            "AccountSnapshotRecorded",
            &format!(
                r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":{multiplier},"equity":"100","cash":"100","buying_power":"100","non_marginable_buying_power":"100"{members},"risk_clock":"{REGULAR}"}}"#
            ),
        );
        gate_over(&rows, Some(config())).check(&opening("3", "99.95"))
    };
    assert_eq!(poor(1, "none"), denied("insufficient_settled_buying_power"));
    assert_eq!(poor(2, "none"), denied("insufficient_buying_power"));
    assert_eq!(
        poor(2, "\"cash\""),
        denied("insufficient_settled_buying_power")
    );
    assert_eq!(poor(1, "\"margin\""), denied("insufficient_buying_power"));
    assert_eq!(poor(2, "null"), denied("insufficient_buying_power"));
}

/// Crypto never counts toward the day-trade budget (trading spec §9.2), so a crypto opening's
/// budget is evaluated even on a version 1 snapshot (DEC-2811).
#[test]
fn a_crypto_openings_budget_is_evaluated_without_declared_facts() {
    let gate = gate_over(&journaled_with(&RICH), Some(config()));
    let mut crypto = opening("3", "99.95");
    crypto.asset_class = AssetClass::Crypto;
    let advisory = gate.advisory(&crypto);
    if let Ok(advisory) = &advisory {
        assert!(
            !advisory.deferred.contains(&Deferred::DayTradeBudget),
            "{advisory:?}"
        );
    }
    let equity = gate.advisory(&opening("3", "99.95")).unwrap();
    assert!(equity.deferred.contains(&Deferred::DayTradeBudget));
    assert!(
        advisory.is_ok(),
        "the crypto opening is evaluated: {advisory:?}"
    );
}

/// Two hours after [`REGULAR`], still in the regular session: the instant the cooldown cases ask.
const LATER: &str = "2026-10-26T17:00:00.000000000Z";

/// The demo's re-entry cooldown, `reentry_cooldown_s` (mandate spec §5.3).
const COOLDOWN_S: i64 = 3600;

fn instant(stamp: &str) -> UtcNanos {
    UtcNanos::parse(stamp).unwrap()
}

/// [`journaled_with`]'s account, ticked and marked again at [`LATER`], so its facts are fresh then.
fn at_later(mut rows: Rows) -> Rows {
    rows.push(CLOCK, "ClockAdvanced", &format!(r#"{{"clock":"{LATER}"}}"#));
    rows.push(
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","risk_clock":"{LATER}"}}"#
        ),
    );
    rows.push(
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{SPY}","price":"100.05","source":"quote","feed":"fixture","bid":"99.95","sane":true,"session":"regular","risk_clock":"{LATER}"}}"#
        ),
    );
    rows
}

/// One order of the agent's in SPY, `side` for `qty`, filled whole at `at`, with `purpose` as the
/// executor journals it.
fn filled(rows: &mut Rows, order: &str, (side, qty): (&str, u32), purpose: &str, at: &str) {
    rows.push(
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"{purpose}","risk_clock":"{at}"}}"#),
    );
    rows.push(
        ACCOUNT,
        "OrderSubmitted",
        &format!(r#"{{"client_order_id":"{order}","instrument_id":"{SPY}","side":"{side}","risk_clock":"{at}"}}"#),
    );
    rows.push(
        ACCOUNT,
        "FillApplied",
        &format!(
            r#"{{"fill_id":"f-{order}","client_order_id":"{order}","instrument_id":"{SPY}","side":"{side}","qty_gross":"{qty}","price":"99.95","trade_date":"2026-10-26","risk_clock":"{at}","fees":[]}}"#
        ),
    );
    rows.push(
        ACCOUNT,
        "OrderStateChanged",
        &format!(
            r#"{{"client_order_id":"{order}","state":"filled","replaced_by":null,"ignored":false,"risk_clock":"{at}"}}"#
        ),
    );
}

/// `offset` seconds after [`REGULAR`], as the journal stamps it.
fn after_regular(offset: i64) -> String {
    UtcNanos::from_parts(instant(REGULAR).secs() + offset, 0)
        .unwrap()
        .to_string()
}

/// Mandate spec §5.3, check 2: no opening until `reentry_cooldown_s` after the agent's last exit
/// fill in the instrument. An owner exit's fill is an exit fill (lane DR's rehearsal, 2026-10-27:
/// the runtime asked to buy back in the second its owner exit filled). The dry run reads the last
/// exit fill from the same fold the binding gate reads it from, so it asks nothing the binding
/// gate refuses.
#[test]
fn an_opening_inside_the_reentry_cooldown_after_an_owner_exit_is_denied() {
    let late = after_regular(2 * COOLDOWN_S - 60);
    let mut inside = journaled_with(&RICH);
    filled(&mut inside, "o1", ("buy", 3), "open", &after_regular(60));
    filled(&mut inside, "o2", ("sell", 3), "owner_exit", &late);
    let inside = at_later(inside);
    assert_eq!(
        gate_over(&inside, Some(config())).check(&opening("3", "99.95")),
        denied("reentry_cooldown"),
        "a minute after the owner exit filled"
    );
    let mut outside = journaled_with(&RICH);
    filled(&mut outside, "o1", ("buy", 3), "open", &after_regular(60));
    filled(
        &mut outside,
        "o2",
        ("sell", 3),
        "owner_exit",
        &after_regular(120),
    );
    let outside = at_later(outside);
    assert_eq!(
        gate_over(&outside, Some(config())).check(&opening("3", "99.95")),
        asked(),
        "the cooldown has passed"
    );
}

/// DEC-2819 item 4, DEC-2143: once the account has traded, a research agent's every opening is
/// held (`research_agent_traded`), as the binding gate holds it; and a pinned agent's while it
/// still holds a position (`account_traded`). Its exits never read the hold.
#[test]
fn the_dry_run_holds_an_opening_as_the_binding_gate_does_once_the_account_has_traded() {
    let mut holding = journaled_with(&RICH);
    filled(&mut holding, "o1", ("buy", 3), "open", &after_regular(60));
    let holding = at_later(holding);
    assert_eq!(
        gate_over(&holding, Some(config())).check(&opening("3", "99.95")),
        denied("account_traded")
    );
    let research = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/cell/demo/research-mandate.json"
    ))
    .unwrap();
    let research = Mandate::parse(&mandate_canon::parse(&research).unwrap()).unwrap();
    let mut flat = journaled_with(&RICH);
    filled(&mut flat, "o1", ("buy", 3), "open", &after_regular(60));
    filled(
        &mut flat,
        "o2",
        ("sell", 3),
        "owner_exit",
        &after_regular(120),
    );
    let flat = at_later(flat);
    let facts = Facts::new(CLOCK, AGENT);
    facts.follow(ACCOUNT);
    facts.feed(&flat.0);
    let gate = Gate::new(facts, (DEPLOYED, research), Some(config()), FRESH);
    assert_eq!(
        gate.check(&opening("1", "99.95")),
        denied("research_agent_traded")
    );
    let exit = Proposal {
        side: Side::Sell,
        purpose: Purpose::DiscretionaryExit,
        ..opening("1", "99.95")
    };
    assert_eq!(
        gate.check(&exit),
        DryRunVerdict::Allow,
        "an exit never reads it"
    );
}

proptest! {
    /// The dry run's answer to an opening equals the spec's, computed here on its own from the
    /// fills alone, over interleaved buys and exits at random instants: held while a position is
    /// (DEC-2143), else denied `reentry_cooldown` within `reentry_cooldown_s` of the last sell fill
    /// (mandate spec §5.3), else asked.
    #[test]
    fn the_dry_runs_reentry_answer_is_the_specs(
        trades in prop::collection::vec((any::<bool>(), 1u32..4, 0i64..7000), 1..8)
    ) {
        let mut rows = journaled_with(&RICH);
        let mut held = 0u32;
        let mut last_sell: Option<i64> = None;
        let mut sorted = trades.clone();
        sorted.sort_by_key(|(_, _, at)| *at);
        for (n, (buy, qty, at)) in sorted.into_iter().enumerate() {
            let (side, qty, purpose) = if buy || held == 0 {
                ("buy", qty, "open")
            } else {
                ("sell", qty.min(held), "owner_exit")
            };
            filled(&mut rows, &format!("o{n}"), (side, qty), purpose, &after_regular(at));
            if side == "buy" {
                held += qty;
            } else {
                held -= qty;
                last_sell = Some(at);
            }
        }
        let rows = at_later(rows);
        let now = 2 * COOLDOWN_S;
        let expected = if held > 0 {
            denied("account_traded")
        } else if last_sell.is_some_and(|at| now - at < COOLDOWN_S) {
            denied("reentry_cooldown")
        } else {
            asked()
        };
        prop_assert_eq!(gate_over(&rows, Some(config())).check(&opening("3", "99.95")), expected);
    }
}
