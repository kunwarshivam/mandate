//! The fail-closed rules of DEC-2378 item 3, one test a rule: every fact an opening needs is
//! journaled, current by the risk clock, and readable, or the opening is held.

use mandate_canon::{Int, Key, Value};
use mandate_gate_context::{AccountFacts, Freshness, Held, Journaled, Mark, Row};
use mandate_num::{Price, Usd};
use mandate_time::UtcNanos;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const OTHER: &str = "acct:ws1:alpaca:other";
const CONTROL: &str = "ctl:ws1";
const CLOCK: &str = "clock:ws1";
const AGENT: &str = "agent-1";
const ASSET: &str = "us_equity:AAPL";
const FRESH: Freshness = Freshness {
    mark_max_age_s: 60,
    snapshot_max_age_s: 600,
};
const START: i64 = 1_790_000_000;

trait Members {
    fn with(self, member: &str, value: Value) -> Self;
    fn without(self, member: &str) -> Self;
}

impl Members for Value {
    fn with(mut self, member: &str, value: Value) -> Self {
        if let Value::Object(members) = &mut self {
            members.insert(Key::new(member).unwrap(), value);
        }
        self
    }

    fn without(mut self, member: &str) -> Self {
        if let Value::Object(members) = &mut self {
            members.remove(member);
        }
        self
    }
}

fn object(members: &[(&str, Value)]) -> Value {
    members
        .iter()
        .fold(Value::Object(Default::default()), |object, (key, value)| {
            object.with(key, value.clone())
        })
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn instant(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn clock(secs: i64) -> Value {
    text(&instant(secs).to_string())
}

fn deployed(agent: &str, version: &str) -> Value {
    object(&[
        ("agent_id", text(agent)),
        ("mandate_version", text(version)),
    ])
}

fn applied(agent: &str, version: &str, result: &str) -> Value {
    object(&[
        ("agent_id", text(agent)),
        ("old_version", text("sha256:v1")),
        ("new_version", text(version)),
        ("result", text(result)),
        ("risk_clock", clock(START)),
    ])
}

fn snapshot(equity: &str, at: i64) -> Value {
    let flag = |set| Value::Bool(set);
    object(&[
        ("status", text("ACTIVE")),
        ("crypto_status", text("INACTIVE")),
        ("trading_blocked", flag(false)),
        ("account_blocked", flag(false)),
        ("trade_suspended_by_user", flag(true)),
        ("multiplier", Value::Int(Int::new(2).unwrap())),
        ("equity", text(equity)),
        ("cash", text("400.25")),
        ("buying_power", text("800.5")),
        ("non_marginable_buying_power", text("400.25")),
        ("model_cash", Value::Null),
        ("risk_clock", clock(at)),
    ])
}

fn mark_v1(asset: &str, at: i64) -> Value {
    object(&[
        ("instrument_id", text(asset)),
        ("price", text("150")),
        ("source", text("quote")),
        ("feed", text("iex")),
        ("risk_clock", clock(at)),
    ])
}

fn mark(asset: &str, bid: Option<&str>, sane: bool, session: &str, at: i64) -> Value {
    mark_v1(asset, at)
        .with("bid", bid.map_or(Value::Null, text))
        .with("sane", Value::Bool(sane))
        .with("session", text(session))
}

fn regular(bid: &str, at: i64) -> Value {
    mark(ASSET, Some(bid), true, "regular", at)
}

fn tick(at: i64) -> Value {
    object(&[("clock", clock(at))])
}

fn on(state: &mut Journaled, stream: &str, event_type: &str, payload: &Value) {
    let row = Row {
        stream,
        event_type,
        payload,
    };
    state.fold(row).unwrap();
}

fn acct(state: &mut Journaled, event_type: &str, payload: &Value) {
    on(state, ACCOUNT, event_type, payload);
}

fn deploy(state: &mut Journaled) {
    on(
        state,
        CONTROL,
        "AgentDeployed",
        &deployed(AGENT, "sha256:v1"),
    );
}

/// A deployed agent with a snapshot and a sane regular-session mark, all at `START`.
fn ready() -> Journaled {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START));
    deploy(&mut state);
    acct(
        &mut state,
        "AccountSnapshotRecorded",
        &snapshot("1000.75", START),
    );
    acct(&mut state, "MarkUpdated", &regular("189.5", START));
    state
}

fn version(state: &Journaled) -> String {
    state.facts(ASSET, FRESH).unwrap().mandate_version
}

#[test]
fn every_fact_journaled_and_current_is_read_exactly() {
    let facts = ready().facts(ASSET, FRESH).unwrap();
    assert_eq!(facts.now, instant(START));
    assert_eq!(facts.mandate_version, "sha256:v1");
    let bid = Price::parse("189.5").unwrap();
    assert_eq!(
        facts.mark,
        Mark {
            bid,
            at: instant(START)
        }
    );
    let usd = |text| Usd::parse(text).unwrap();
    let account = AccountFacts {
        status: "ACTIVE".to_owned(),
        crypto_status: "INACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: true,
        multiplier: 2,
        equity: usd("1000.75"),
        cash: usd("400.25"),
        buying_power: usd("800.5"),
        non_marginable_buying_power: usd("400.25"),
        at: instant(START),
    };
    assert_eq!(facts.account, account);
}

#[test]
fn nothing_journaled_holds_for_want_of_a_clock() {
    let state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    assert_eq!(state.now(), None);
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoClock));
}

#[test]
fn no_deployment_of_this_agent_holds_for_want_of_a_mandate() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START));
    acct(
        &mut state,
        "AccountSnapshotRecorded",
        &snapshot("1000", START),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMandate));
    on(
        &mut state,
        CONTROL,
        "AgentDeployed",
        &deployed("agent-2", "sha256:v9"),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMandate));
}

#[test]
fn an_applied_version_is_the_mandate_in_force_and_a_rejected_one_is_not() {
    let mut state = ready();
    acct(
        &mut state,
        "MandateVersionApplied",
        &applied(AGENT, "sha256:v2", "rejected"),
    );
    assert_eq!(version(&state), "sha256:v1");
    acct(
        &mut state,
        "MandateVersionApplied",
        &applied("agent-2", "sha256:v3", "applied"),
    );
    assert_eq!(version(&state), "sha256:v1");
    let elsewhere = applied(AGENT, "sha256:v4", "applied");
    on(&mut state, OTHER, "MandateVersionApplied", &elsewhere);
    assert_eq!(version(&state), "sha256:v1");
    acct(
        &mut state,
        "MandateVersionApplied",
        &applied(AGENT, "sha256:v2", "applied"),
    );
    assert_eq!(version(&state), "sha256:v2");
}

#[test]
fn a_stopped_agent_has_no_mandate_to_open_under() {
    let mut state = ready();
    let stopped = |agent| object(&[("agent_id", text(agent))]);
    on(&mut state, CONTROL, "AgentStopped", &stopped("agent-2"));
    assert!(state.facts(ASSET, FRESH).is_ok());
    on(&mut state, CONTROL, "AgentStopped", &stopped(AGENT));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMandate));
}

#[test]
fn no_snapshot_of_this_account_holds() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    deploy(&mut state);
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoSnapshot));
    on(
        &mut state,
        OTHER,
        "AccountSnapshotRecorded",
        &snapshot("1", START),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoSnapshot));
}

#[test]
fn the_latest_snapshot_is_read() {
    let mut state = ready();
    acct(
        &mut state,
        "AccountSnapshotRecorded",
        &snapshot("900", START + 5),
    );
    let account = state.facts(ASSET, FRESH).unwrap().account;
    assert_eq!(account.equity, Usd::parse("900").unwrap());
    assert_eq!(account.at, instant(START + 5));
}

#[test]
fn a_snapshot_is_stale_once_its_age_reaches_the_limit() {
    let mut state = ready();
    let max = i64::from(FRESH.snapshot_max_age_s);
    acct(
        &mut state,
        "MarkUpdated",
        &regular("189.5", START + max - 1),
    );
    assert!(state.facts(ASSET, FRESH).is_ok());
    acct(&mut state, "MarkUpdated", &regular("189.5", START + max));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleSnapshot));
}

#[test]
fn no_mark_for_the_instrument_on_this_account_holds() {
    let mut state = ready();
    assert_eq!(state.facts("us_equity:MSFT", FRESH), Err(Held::NoMark));
    let elsewhere = mark("us_equity:MSFT", Some("10"), true, "regular", START);
    on(&mut state, OTHER, "MarkUpdated", &elsewhere);
    assert_eq!(state.facts("us_equity:MSFT", FRESH), Err(Held::NoMark));
}

#[test]
fn a_mark_is_stale_once_its_age_reaches_the_limit() {
    let mut state = ready();
    let max = i64::from(FRESH.mark_max_age_s);
    acct(&mut state, "ClockAdvanced", &tick(START + max - 1));
    assert!(state.facts(ASSET, FRESH).is_ok());
    acct(&mut state, "ClockAdvanced", &tick(START + max));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleMark));
}

#[test]
fn a_mark_that_is_not_sane_holds_until_a_sane_one_follows() {
    let mut state = ready();
    acct(
        &mut state,
        "MarkUpdated",
        &mark(ASSET, Some("189"), false, "regular", START + 1),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::InsaneMark));
    acct(
        &mut state,
        "MarkUpdated",
        &mark(ASSET, None, false, "crypto", START + 2),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::InsaneMark));
    acct(&mut state, "MarkUpdated", &regular("191", START + 3));
    let mark = state.facts(ASSET, FRESH).unwrap().mark;
    assert_eq!(mark.bid, Price::parse("191").unwrap());
    assert_eq!(mark.at, instant(START + 3));
}

#[test]
fn a_crypto_mark_counts_at_any_hour() {
    let mut state = ready();
    let pair = "crypto:BTC/USD";
    acct(
        &mut state,
        "MarkUpdated",
        &mark(pair, Some("65000"), true, "crypto", START + 1),
    );
    let bid = state.facts(pair, FRESH).unwrap().mark.bid;
    assert_eq!(bid, Price::parse("65000").unwrap());
}

#[test]
fn a_mark_outside_the_regular_session_is_ignored_sane_or_not() {
    for session in ["overnight", "pre_market", "after_hours"] {
        let mut state = ready();
        acct(
            &mut state,
            "MarkUpdated",
            &mark(ASSET, Some("1"), true, session, START + 1),
        );
        acct(
            &mut state,
            "MarkUpdated",
            &mark(ASSET, Some("2"), false, session, START + 2),
        );
        let facts = state.facts(ASSET, FRESH).unwrap();
        assert_eq!(facts.mark.bid, Price::parse("189.5").unwrap(), "{session}");
        assert_eq!(facts.now, instant(START + 2), "{session}");
    }
}

#[test]
fn a_version_one_mark_has_no_verdict_and_restarts_nothing() {
    let mut state = ready();
    let max = i64::from(FRESH.mark_max_age_s);
    acct(&mut state, "MarkUpdated", &mark_v1(ASSET, START + max));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleMark));
    let mut fresh = Journaled::new(ACCOUNT, CLOCK, AGENT);
    on(&mut fresh, CLOCK, "ClockAdvanced", &tick(START));
    deploy(&mut fresh);
    acct(&mut fresh, "AccountSnapshotRecorded", &snapshot("1", START));
    acct(&mut fresh, "MarkUpdated", &mark_v1(ASSET, START));
    assert_eq!(fresh.facts(ASSET, FRESH), Err(Held::NoMark));
}

#[test]
fn the_clock_is_the_latest_account_stream_risk_clock_alone() {
    let mut state = ready();
    on(&mut state, CONTROL, "ClockAdvanced", &tick(START + 500));
    on(
        &mut state,
        OTHER,
        "AccountSnapshotRecorded",
        &snapshot("1", START + 500),
    );
    assert_eq!(state.now(), Some(instant(START)));
    let changed = object(&[("risk_clock", clock(START + 7))]);
    acct(&mut state, "GateDecided", &changed);
    assert_eq!(state.now(), Some(instant(START + 7)));
    acct(&mut state, "GateDecided", &object(&[]));
    assert_eq!(state.now(), Some(instant(START + 7)));
}

/// Each row of a type the fold reads, broken in one member, is refused and holds every later
/// opening: a row the fold could not read may have changed a fact, so nothing it holds is trusted.
/// A clock that goes back is refused the same way.
#[test]
fn an_unreadable_row_of_a_type_it_reads_holds_from_then_on() {
    let snap = snapshot("1", START);
    let cases = [
        (ACCOUNT, "ClockAdvanced", tick(START - 1)),
        (
            ACCOUNT,
            "GateDecided",
            object(&[("risk_clock", text("2026-10-26"))]),
        ),
        (
            ACCOUNT,
            "MarkUpdated",
            regular("1", START).with("bid", text("0")),
        ),
        (
            ACCOUNT,
            "MarkUpdated",
            mark(ASSET, None, true, "regular", START),
        ),
        (
            ACCOUNT,
            "MarkUpdated",
            regular("1", START).with("session", text("midday")),
        ),
        (
            ACCOUNT,
            "MarkUpdated",
            regular("1", START).with("sane", text("true")),
        ),
        (
            ACCOUNT,
            "MarkUpdated",
            regular("1", START).without("instrument_id"),
        ),
        (
            ACCOUNT,
            "MandateVersionApplied",
            applied(AGENT, "sha256:v2", "maybe"),
        ),
        (
            ACCOUNT,
            "MandateVersionApplied",
            applied(AGENT, "x", "applied").without("risk_clock"),
        ),
        (ACCOUNT, "AccountSnapshotRecorded", snapshot("lots", START)),
        (
            ACCOUNT,
            "AccountSnapshotRecorded",
            snap.clone().with("account_blocked", Value::Null),
        ),
        (
            ACCOUNT,
            "AccountSnapshotRecorded",
            snap.clone().without("multiplier"),
        ),
        (
            ACCOUNT,
            "AccountSnapshotRecorded",
            snap.without("risk_clock"),
        ),
        (
            CONTROL,
            "AgentDeployed",
            deployed(AGENT, "v").without("mandate_version"),
        ),
        (CONTROL, "AgentStopped", object(&[])),
    ];
    for (stream, event_type, payload) in cases {
        let mut state = ready();
        let row = Row {
            stream,
            event_type,
            payload: &payload,
        };
        assert!(state.fold(row).is_err(), "{event_type} {payload:?}");
        assert_eq!(
            state.facts(ASSET, FRESH),
            Err(Held::Unreadable),
            "{event_type}"
        );
        acct(
            &mut state,
            "AccountSnapshotRecorded",
            &snapshot("1", START + 1),
        );
        assert_eq!(
            state.facts(ASSET, FRESH),
            Err(Held::Unreadable),
            "{event_type}"
        );
    }
}

/// R1's probe on #2432: the account stream goes quiet while the scheduler keeps ticking, so the
/// mark and the snapshot age by the scheduler's clock.
#[test]
fn scheduler_ticks_age_the_facts_while_the_account_stream_is_quiet() {
    let mut state = ready();
    let max = i64::from(FRESH.mark_max_age_s);
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START + max - 1));
    assert!(state.facts(ASSET, FRESH).is_ok());
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START + max));
    assert_eq!(state.now(), Some(instant(START + max)));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleMark));
    let snapshot_max = i64::from(FRESH.snapshot_max_age_s);
    on(
        &mut state,
        CLOCK,
        "ClockAdvanced",
        &tick(START + snapshot_max),
    );
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::StaleSnapshot));
}

#[test]
fn the_scheduler_alone_is_a_clock() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START));
    assert_eq!(state.now(), Some(instant(START)));
    on(
        &mut state,
        CLOCK,
        "ClockOffsetRecorded",
        &object(&[("clock", text("x"))]),
    );
    on(&mut state, CLOCK, "TradingDayStarted", &object(&[]));
    assert_eq!(state.now(), Some(instant(START)));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoMandate));
}

/// Each stream's clock only moves forward against its own: an account row behind the
/// scheduler's tick is no clock going back, and the later of the two is the time.
#[test]
fn an_account_row_behind_the_scheduler_is_read_at_its_own_clock() {
    let mut state = ready();
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START + 30));
    acct(&mut state, "MarkUpdated", &regular("190", START + 10));
    let facts = state.facts(ASSET, FRESH).unwrap();
    assert_eq!(facts.now, instant(START + 30));
    assert_eq!(facts.mark.at, instant(START + 10));
}

#[test]
fn a_scheduler_tick_that_goes_back_or_does_not_read_holds_from_then_on() {
    for payload in [
        tick(START - 1),
        object(&[("clock", text("soon"))]),
        object(&[]),
    ] {
        let mut state = ready();
        on(&mut state, CLOCK, "ClockAdvanced", &tick(START));
        let row = Row {
            stream: CLOCK,
            event_type: "ClockAdvanced",
            payload: &payload,
        };
        assert!(state.fold(row).is_err(), "{payload:?}");
        on(&mut state, CLOCK, "ClockAdvanced", &tick(START + 1));
        assert_eq!(
            state.facts(ASSET, FRESH),
            Err(Held::Unreadable),
            "{payload:?}"
        );
    }
}

/// R1 on #2432: an opening needs the scheduler's clock. The account stream's clock alone stops
/// when the stream is quiet, so it never admits an opening by itself.
#[test]
fn the_account_streams_clock_alone_admits_no_opening() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    deploy(&mut state);
    acct(
        &mut state,
        "AccountSnapshotRecorded",
        &snapshot("1000", START),
    );
    acct(&mut state, "MarkUpdated", &regular("189.5", START));
    assert_eq!(state.now(), Some(instant(START)));
    assert_eq!(state.facts(ASSET, FRESH), Err(Held::NoClock));
    on(&mut state, CLOCK, "ClockAdvanced", &tick(START));
    assert!(state.facts(ASSET, FRESH).is_ok());
}

/// R1 on #2447: the mandate in force does not depend on which stream reaches the fold first. A
/// version applied on the account stream, folded before the control stream's deployment, stays
/// the one in force; a stop is final whichever came first.
#[test]
fn the_mandate_in_force_is_the_same_whichever_stream_is_folded_first() {
    let applied_first = |stop: bool| {
        let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
        on(&mut state, CLOCK, "ClockAdvanced", &tick(START));
        acct(
            &mut state,
            "AccountSnapshotRecorded",
            &snapshot("1000", START),
        );
        acct(&mut state, "MarkUpdated", &regular("189.5", START));
        let version = applied(AGENT, "sha256:v2", "applied");
        acct(&mut state, "MandateVersionApplied", &version);
        if stop {
            let stopped = object(&[("agent_id", text(AGENT))]);
            on(&mut state, CONTROL, "AgentStopped", &stopped);
        }
        deploy(&mut state);
        state
    };
    assert_eq!(version(&applied_first(false)), "sha256:v2");
    assert_eq!(
        applied_first(true).facts(ASSET, FRESH),
        Err(Held::NoMandate)
    );
}

fn registered(kind: &str, model: Option<(&str, &str)>, hash: &str) -> Value {
    let nullable = |value: Option<&str>| value.map_or(Value::Null, text);
    object(&[
        ("kind", text(kind)),
        ("content_hash", text(hash)),
        ("model_id", nullable(model.map(|(id, _)| id))),
        ("model_version", nullable(model.map(|(_, version)| version))),
        ("params", Value::Array(Vec::new())),
        ("admits_instruments", Value::Null),
    ])
}

/// The models the workspace registered, by id and version, with their content hashes: the
/// builder sizes only outputs of a model the registry hashes (DEC-2378 item 10). Another kind of
/// snapshot registers no model, and a model registered again keeps its latest hash.
#[test]
fn the_registered_models_are_folded_from_their_snapshots() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    let ma = Some(("quant.ma_crossover", "1.0.0"));
    on(
        &mut state,
        CONTROL,
        "ConfigSnapshotRegistered",
        &registered("model_version", ma, "sha256:aa"),
    );
    on(
        &mut state,
        CONTROL,
        "ConfigSnapshotRegistered",
        &registered("rule_set", None, "sha256:bb"),
    );
    let expected = [(
        ("quant.ma_crossover".to_owned(), "1.0.0".to_owned()),
        "sha256:aa".to_owned(),
    )];
    assert_eq!(state.models(), expected.clone().into());
    on(
        &mut state,
        CONTROL,
        "ConfigSnapshotRegistered",
        &registered("model_version", ma, "sha256:cc"),
    );
    let latest = [(
        ("quant.ma_crossover".to_owned(), "1.0.0".to_owned()),
        "sha256:cc".to_owned(),
    )];
    assert_eq!(state.models(), latest.into());
}

#[test]
fn a_model_snapshot_without_its_id_version_or_hash_is_refused() {
    for payload in [
        registered("model_version", None, "sha256:aa"),
        registered("model_version", Some(("m", "1")), "sha256:aa").without("content_hash"),
        registered("model_version", Some(("m", "1")), "sha256:aa")
            .with("model_version", Value::Null),
        object(&[("content_hash", text("sha256:aa"))]),
    ] {
        let mut state = ready();
        let row = Row {
            stream: CONTROL,
            event_type: "ConfigSnapshotRegistered",
            payload: &payload,
        };
        assert!(state.fold(row).is_err(), "{payload:?}");
        assert_eq!(state.facts(ASSET, FRESH), Err(Held::Unreadable));
    }
}
