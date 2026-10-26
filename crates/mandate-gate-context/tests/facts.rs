//! The fail-closed rules of DEC-2378 item 3, one test a rule: every fact an opening needs is
//! journaled, current by the risk clock, and readable, or the opening is held.

use std::collections::BTreeMap;

use mandate_canon::{Key, Value};
use mandate_gate_context::{AccountFacts, Freshness, Held, Journaled, Mark, Row};
use mandate_num::{Price, Usd};
use mandate_time::UtcNanos;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const CONTROL: &str = "ctl:ws1";
const AGENT: &str = "agent-1";
const ASSET: &str = "us_equity:AAPL";
const FRESH: Freshness = Freshness {
    mark_max_age_s: 60,
    snapshot_max_age_s: 600,
};
const START: i64 = 1_790_000_000;

fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(key, value)| (Key::new(key).unwrap(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn stamp(secs: i64) -> String {
    UtcNanos::from_parts(secs, 0).unwrap().to_string()
}

fn deployed(agent: &str, version: &str) -> Value {
    object(&[
        ("agent_id", text(agent)),
        ("mandate_version", text(version)),
        ("record_ref", text("sha256:record")),
    ])
}

fn applied(agent: &str, version: &str, result: &str, at: i64) -> Value {
    object(&[
        ("agent_id", text(agent)),
        ("old_version", text("sha256:v1")),
        ("new_version", text(version)),
        ("classification", text("risk_reducing")),
        ("result", text(result)),
        ("risk_clock", text(&stamp(at))),
    ])
}

fn stopped(agent: &str) -> Value {
    object(&[("agent_id", text(agent)), ("reason", text("owner"))])
}

fn snapshot(equity: &str, at: i64) -> Value {
    object(&[
        ("status", text("ACTIVE")),
        ("crypto_status", text("INACTIVE")),
        ("trading_blocked", Value::Bool(false)),
        ("account_blocked", Value::Bool(false)),
        ("trade_suspended_by_user", Value::Bool(true)),
        (
            "multiplier",
            Value::Int(mandate_canon::Int::new(2).unwrap()),
        ),
        ("equity", text(equity)),
        ("cash", text("400.25")),
        ("buying_power", text("800.50")),
        ("non_marginable_buying_power", text("400.25")),
        ("accrued_fees", text("0")),
        ("model_cash", Value::Null),
        ("cash_band", Value::Null),
        ("cash_in_band", Value::Null),
        ("risk_clock", text(&stamp(at))),
    ])
}

fn mark(asset: &str, bid: Option<&str>, sane: bool, session: &str, at: i64) -> Value {
    object(&[
        ("instrument_id", text(asset)),
        ("price", text("190")),
        ("source", text("quote")),
        ("feed", text("iex")),
        ("bid", bid.map_or(Value::Null, text)),
        ("sane", Value::Bool(sane)),
        ("session", text(session)),
        ("risk_clock", text(&stamp(at))),
    ])
}

fn mark_v1(asset: &str, at: i64) -> Value {
    object(&[
        ("instrument_id", text(asset)),
        ("price", text("150")),
        ("source", text("quote")),
        ("feed", text("iex")),
        ("risk_clock", text(&stamp(at))),
    ])
}

fn tick(at: i64) -> Value {
    object(&[("clock", text(&stamp(at)))])
}

fn fold(state: &mut Journaled, stream: &str, event_type: &str, payload: &Value) {
    state
        .fold(Row {
            stream,
            event_type,
            payload,
        })
        .unwrap();
}

/// A deployed agent with a snapshot and a sane regular-session mark, all at `START`.
fn ready() -> Journaled {
    let mut state = Journaled::new(ACCOUNT, AGENT);
    fold(
        &mut state,
        CONTROL,
        "AgentDeployed",
        &deployed(AGENT, "sha256:v1"),
    );
    fold(
        &mut state,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &snapshot("1000.75", START),
    );
    fold(
        &mut state,
        ACCOUNT,
        "MarkUpdated",
        &mark(ASSET, Some("189.5"), true, "regular", START),
    );
    state
}

#[test]
fn every_fact_journaled_and_current_is_read_exactly() {
    let facts = ready().facts(ASSET, FRESH).unwrap();
    assert_eq!(facts.now, UtcNanos::from_parts(START, 0).unwrap());
    assert_eq!(facts.mandate_version, "sha256:v1");
    assert_eq!(
        facts.mark,
        Mark {
            bid: Price::parse("189.5").unwrap(),
            at: UtcNanos::from_parts(START, 0).unwrap(),
        }
    );
    assert_eq!(
        facts.account,
        AccountFacts {
            status: "ACTIVE".to_owned(),
            crypto_status: "INACTIVE".to_owned(),
            trading_blocked: false,
            account_blocked: false,
            trade_suspended_by_user: true,
            multiplier: 2,
            equity: Usd::parse("1000.75").unwrap(),
            cash: Usd::parse("400.25").unwrap(),
            buying_power: Usd::parse("800.50").unwrap(),
            non_marginable_buying_power: Usd::parse("400.25").unwrap(),
            at: UtcNanos::from_parts(START, 0).unwrap(),
        }
    );
}

#[test]
fn nothing_journaled_holds_for_want_of_a_clock() {
    let state = Journaled::new(ACCOUNT, AGENT);
    assert_eq!(state.now(), None);
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoClock));
}

#[test]
fn no_deployment_holds_for_want_of_a_mandate() {
    let mut state = Journaled::new(ACCOUNT, AGENT);
    fold(
        &mut state,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &snapshot("1000", START),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMandate));
}

#[test]
fn another_agents_deployment_is_not_this_agents_mandate() {
    let mut state = Journaled::new(ACCOUNT, AGENT);
    fold(
        &mut state,
        CONTROL,
        "AgentDeployed",
        &deployed("agent-2", "sha256:v9"),
    );
    fold(
        &mut state,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &snapshot("1000", START),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMandate));
}

#[test]
fn an_applied_version_is_the_mandate_in_force_and_a_rejected_one_is_not() {
    let mut state = ready();
    fold(
        &mut state,
        ACCOUNT,
        "MandateVersionApplied",
        &applied(AGENT, "sha256:v2", "rejected", START),
    );
    assert_eq!(
        state.facts(ASSET, FRESH).unwrap().mandate_version,
        "sha256:v1"
    );
    fold(
        &mut state,
        ACCOUNT,
        "MandateVersionApplied",
        &applied("agent-2", "sha256:v3", "applied", START),
    );
    assert_eq!(
        state.facts(ASSET, FRESH).unwrap().mandate_version,
        "sha256:v1"
    );
    fold(
        &mut state,
        ACCOUNT,
        "MandateVersionApplied",
        &applied(AGENT, "sha256:v2", "applied", START),
    );
    assert_eq!(
        state.facts(ASSET, FRESH).unwrap().mandate_version,
        "sha256:v2"
    );
}

#[test]
fn a_version_applied_on_another_stream_is_not_read() {
    let mut state = ready();
    fold(
        &mut state,
        "acct:ws1:alpaca:other",
        "MandateVersionApplied",
        &applied(AGENT, "sha256:v2", "applied", START),
    );
    assert_eq!(
        state.facts(ASSET, FRESH).unwrap().mandate_version,
        "sha256:v1"
    );
}

#[test]
fn a_stopped_agent_has_no_mandate_to_open_under() {
    let mut state = ready();
    fold(&mut state, CONTROL, "AgentStopped", &stopped("agent-2"));
    assert!(state.facts(ASSET, FRESH).is_ok());
    fold(&mut state, CONTROL, "AgentStopped", &stopped(AGENT));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMandate));
}

#[test]
fn no_snapshot_holds() {
    let mut state = Journaled::new(ACCOUNT, AGENT);
    fold(
        &mut state,
        CONTROL,
        "AgentDeployed",
        &deployed(AGENT, "sha256:v1"),
    );
    fold(&mut state, ACCOUNT, "ClockAdvanced", &tick(START));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoSnapshot));
}

#[test]
fn a_snapshot_on_another_stream_is_not_this_accounts() {
    let mut state = Journaled::new(ACCOUNT, AGENT);
    fold(
        &mut state,
        CONTROL,
        "AgentDeployed",
        &deployed(AGENT, "sha256:v1"),
    );
    fold(&mut state, ACCOUNT, "ClockAdvanced", &tick(START));
    fold(
        &mut state,
        "acct:ws1:alpaca:other",
        "AccountSnapshotRecorded",
        &snapshot("1000", START),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoSnapshot));
}

#[test]
fn the_latest_snapshot_is_read() {
    let mut state = ready();
    fold(
        &mut state,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &snapshot("900", START + 5),
    );
    let facts = state.facts(ASSET, FRESH).unwrap();
    assert_eq!(facts.account.equity, Usd::parse("900").unwrap());
    assert_eq!(
        facts.account.at,
        UtcNanos::from_parts(START + 5, 0).unwrap()
    );
}

#[test]
fn a_snapshot_is_stale_once_its_age_reaches_the_limit() {
    let mut state = ready();
    let max = i64::from(FRESH.snapshot_max_age_s);
    let fresh_mark = |at| mark(ASSET, Some("189.5"), true, "regular", at);
    fold(
        &mut state,
        ACCOUNT,
        "MarkUpdated",
        &fresh_mark(START + max - 1),
    );
    assert!(state.facts(ASSET, FRESH).is_ok());
    fold(&mut state, ACCOUNT, "MarkUpdated", &fresh_mark(START + max));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleSnapshot));
}

#[test]
fn no_mark_for_the_instrument_holds() {
    let state = ready();
    assert_eq!(state.facts("us_equity:MSFT", FRESH), Err(Held::NoMark));
}

#[test]
fn a_mark_is_stale_once_its_age_reaches_the_limit() {
    let mut state = ready();
    let max = i64::from(FRESH.mark_max_age_s);
    fold(&mut state, ACCOUNT, "ClockAdvanced", &tick(START + max - 1));
    assert!(state.facts(ASSET, FRESH).is_ok());
    fold(&mut state, ACCOUNT, "ClockAdvanced", &tick(START + max));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleMark));
}

#[test]
fn a_mark_that_is_not_sane_holds_until_a_sane_one_follows() {
    let mut state = ready();
    fold(
        &mut state,
        ACCOUNT,
        "MarkUpdated",
        &mark(ASSET, Some("189"), false, "regular", START + 1),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::InsaneMark));
    fold(
        &mut state,
        ACCOUNT,
        "MarkUpdated",
        &mark(ASSET, None, false, "crypto", START + 2),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::InsaneMark));
    fold(
        &mut state,
        ACCOUNT,
        "MarkUpdated",
        &mark(ASSET, Some("191"), true, "regular", START + 3),
    );
    let facts = state.facts(ASSET, FRESH).unwrap();
    assert_eq!(facts.mark.bid, Price::parse("191").unwrap());
    assert_eq!(facts.mark.at, UtcNanos::from_parts(START + 3, 0).unwrap());
}

#[test]
fn a_crypto_mark_counts_at_any_hour() {
    let mut state = ready();
    fold(
        &mut state,
        ACCOUNT,
        "MarkUpdated",
        &mark("crypto:BTC/USD", Some("65000"), true, "crypto", START + 1),
    );
    assert_eq!(
        state.facts("crypto:BTC/USD", FRESH).unwrap().mark.bid,
        Price::parse("65000").unwrap()
    );
}

#[test]
fn a_mark_outside_the_regular_session_is_ignored_sane_or_not() {
    for session in ["overnight", "pre_market", "after_hours"] {
        let mut state = ready();
        fold(
            &mut state,
            ACCOUNT,
            "MarkUpdated",
            &mark(ASSET, Some("1"), true, session, START + 1),
        );
        fold(
            &mut state,
            ACCOUNT,
            "MarkUpdated",
            &mark(ASSET, Some("2"), false, session, START + 2),
        );
        let facts = state.facts(ASSET, FRESH).unwrap();
        assert_eq!(facts.mark.bid, Price::parse("189.5").unwrap(), "{session}");
        assert_eq!(
            facts.now,
            UtcNanos::from_parts(START + 2, 0).unwrap(),
            "{session}"
        );
    }
}

#[test]
fn a_version_one_mark_has_no_verdict_and_restarts_nothing() {
    let mut state = ready();
    let max = i64::from(FRESH.mark_max_age_s);
    fold(
        &mut state,
        ACCOUNT,
        "MarkUpdated",
        &mark_v1(ASSET, START + max),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleMark));
    let mut fresh = Journaled::new(ACCOUNT, AGENT);
    fold(
        &mut fresh,
        CONTROL,
        "AgentDeployed",
        &deployed(AGENT, "sha256:v1"),
    );
    fold(
        &mut fresh,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &snapshot("1", START),
    );
    fold(&mut fresh, ACCOUNT, "MarkUpdated", &mark_v1(ASSET, START));
    assert_eq!(fresh.facts(ASSET, FRESH), Err(Held::NoMark));
}

#[test]
fn a_mark_on_another_stream_is_not_this_accounts() {
    let mut state = Journaled::new(ACCOUNT, AGENT);
    fold(
        &mut state,
        CONTROL,
        "AgentDeployed",
        &deployed(AGENT, "sha256:v1"),
    );
    fold(
        &mut state,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &snapshot("1", START),
    );
    fold(
        &mut state,
        "acct:ws1:alpaca:other",
        "MarkUpdated",
        &mark(ASSET, Some("10"), true, "regular", START),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMark));
}

#[test]
fn the_clock_is_the_latest_account_stream_risk_clock_alone() {
    let mut state = ready();
    fold(&mut state, CONTROL, "ClockAdvanced", &tick(START + 500));
    fold(
        &mut state,
        "acct:ws1:alpaca:other",
        "AccountSnapshotRecorded",
        &snapshot("1", START + 500),
    );
    assert_eq!(state.now(), Some(UtcNanos::from_parts(START, 0).unwrap()));
    fold(
        &mut state,
        ACCOUNT,
        "OrderStateChanged",
        &object(&[("risk_clock", text(&stamp(START + 7)))]),
    );
    assert_eq!(
        state.now(),
        Some(UtcNanos::from_parts(START + 7, 0).unwrap())
    );
    fold(&mut state, ACCOUNT, "FeesCharged", &object(&[]));
    assert_eq!(
        state.now(),
        Some(UtcNanos::from_parts(START + 7, 0).unwrap())
    );
}

#[test]
fn a_clock_that_goes_back_is_unreadable_and_holds_from_then_on() {
    let mut state = ready();
    let back = state.fold(Row {
        stream: ACCOUNT,
        event_type: "ClockAdvanced",
        payload: &tick(START - 1),
    });
    assert!(back.is_err());
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::Unreadable));
    fold(
        &mut state,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &snapshot("1", START + 1),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::Unreadable));
}

/// Each row of a type the fold reads, broken in one member, is refused and holds every later
/// opening: a row the fold could not read may have changed a fact, so nothing it holds is trusted.
#[test]
fn an_unreadable_row_of_a_type_it_reads_holds_from_then_on() {
    let bad_clock = object(&[("risk_clock", text("2026-10-26"))]);
    let mut no_bid = mark(ASSET, None, true, "regular", START);
    let bad_session = mark(ASSET, Some("1"), true, "midday", START);
    let mut bad_sane = mark(ASSET, Some("1"), true, "regular", START);
    let bad_result = applied(AGENT, "sha256:v2", "maybe", START);
    let bad_equity = snapshot("lots", START);
    let mut bad_flag = snapshot("1", START);
    let mut bad_multiplier = snapshot("1", START);
    if let Value::Object(members) = &mut bad_sane {
        members.insert(Key::new("sane").unwrap(), text("true"));
    }
    if let Value::Object(members) = &mut bad_flag {
        members.insert(Key::new("account_blocked").unwrap(), Value::Null);
    }
    if let Value::Object(members) = &mut bad_multiplier {
        members.remove("multiplier");
    }
    let mut no_version = deployed(AGENT, "sha256:v1");
    if let Value::Object(members) = &mut no_version {
        members.remove("mandate_version");
    }
    if let Value::Object(members) = &mut no_bid {
        members.insert(Key::new("bid").unwrap(), text("0"));
    }
    let cases: [(&str, &str, Value); 10] = [
        (ACCOUNT, "OrderStateChanged", bad_clock),
        (ACCOUNT, "MarkUpdated", no_bid),
        (ACCOUNT, "MarkUpdated", bad_session),
        (ACCOUNT, "MarkUpdated", bad_sane),
        (ACCOUNT, "MandateVersionApplied", bad_result),
        (ACCOUNT, "AccountSnapshotRecorded", bad_equity),
        (ACCOUNT, "AccountSnapshotRecorded", bad_flag),
        (ACCOUNT, "AccountSnapshotRecorded", bad_multiplier),
        (CONTROL, "AgentDeployed", no_version),
        (
            ACCOUNT,
            "AccountSnapshotRecorded",
            snapshot("1", START).without("risk_clock"),
        ),
    ];
    for (stream, event_type, payload) in cases {
        let mut state = ready();
        let refused = state.fold(Row {
            stream,
            event_type,
            payload: &payload,
        });
        assert!(refused.is_err(), "{event_type} {payload:?}");
        assert_eq!(
            state.facts(ASSET, FRESH),
            Err(Held::Unreadable),
            "{event_type}"
        );
    }
}

trait Without {
    fn without(self, member: &str) -> Self;
}

impl Without for Value {
    fn without(mut self, member: &str) -> Self {
        if let Value::Object(members) = &mut self {
            members.remove(member);
        }
        self
    }
}
