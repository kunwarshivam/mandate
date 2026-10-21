//! E6-15 slice B1 (DEC-2301 item 3, DEC-2304): `risk_state::AccountRiskFold` reads the account
//! stream's rows into a risk view, and a row it cannot read makes the view unreadable for good. An
//! unreadable view holds new openings and no exit, protective order, risk exit, or kill switch
//! (`AGENTS.md` rules 3 and 13). Until B2 fills in `fold`, every test that folds a row fails at its
//! `Unimplemented` stub.
//!
//! E6-15 slice B2b-1 (DEC-2305, journal spec §9.30): a version-2 `MarkUpdated` steps the risk state
//! with its verdict, a version-1 mark stays no verdict, and the reference cases' marks and ticks,
//! driven as account-stream rows, give the harness's snapshots and records (DEC-2304 item 6).

use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_domain::{AgentMode, AssetClass, Environment, MarketSession};
use mandate_executor::ExecutorError;
use mandate_executor::risk_state::{
    AccountRiskFold, OrderPath, RiskAppend, RiskCalendar, RiskView, UnreadableCause,
};
use mandate_executor::tripwire::TripwireState;
use mandate_journal::{Draft, StoredEvent};
use mandate_num::{Price, Qty, Usd};
use mandate_spec::context::stop_limit_asset_classes;
use mandate_spec::document::ProvenanceMap;
use mandate_spec::risk::{Opening, RiskState, SessionClock};
use mandate_spec::{Mandate, SpecError, ValidatedMandate, ValidationContext};
use mandate_time::{Date, ExchangeCalendar, Session, UtcNanos};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

const ACCOUNT: &str = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1";
const AGENT: &str = "agent_a";
/// The opening instant, 10:30 New York on a Tuesday, and the risk day it falls in.
const OPENED: &str = "2026-09-22T14:30:00.000000000Z";
const OPENED_DAY: &str = "2026-09-22";

/// Every whole second counts and every instant is the regular session: none of these tests
/// reaches a rule that reads the calendar beyond the risk day's midnight.
struct Wall;

impl SessionClock for Wall {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        to.secs()
            .checked_sub(from.secs())
            .and_then(|seconds| u64::try_from(seconds).ok())
            .ok_or(SpecError::ClockWentBackwards)
    }
}

impl RiskCalendar for Wall {
    fn session(&self, _at: UtcNanos) -> Result<MarketSession, SpecError> {
        Ok(MarketSession::Regular)
    }
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

fn fixture() -> Json {
    serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json")).unwrap()
}

/// `research_equity_pinned`, the reference bases' one pinned US equity, validated.
fn mandate() -> ValidatedMandate {
    validated("research_equity_pinned", &[])
}

/// A reference base with a case's `replace` patch, validated.
fn validated(base: &str, patch: &[Json]) -> ValidatedMandate {
    let mut document = fixture()["bases"][base]["mandate"].clone();
    for change in patch {
        assert_eq!(change["op"], "replace", "the cases here only replace");
        *document
            .pointer_mut(change["path"].as_str().unwrap())
            .unwrap() = change["value"].clone();
    }
    let canonical = parse(&serde_json::to_vec(&document).unwrap()).unwrap();
    let context = ValidationContext {
        account_equity_usd: Usd::parse("25000").unwrap(),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-08-31").unwrap(),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
        stop_limit_asset_classes: stop_limit_asset_classes("alpaca"),
    };
    ValidatedMandate::new(Mandate::parse(&canonical).unwrap(), &context, &[]).unwrap()
}

fn opening() -> Opening {
    Opening {
        position_qty: Qty::ZERO,
        avg_cost: Price::parse("1").unwrap(),
        asset_class: AssetClass::UsEquity,
        at: at(OPENED),
        inherited_loss_usd: Usd::ZERO,
        mark_max_age_s: 60,
    }
}

/// The fold opened after the account stream's `StreamOpened` at seq 1.
fn open<'c>(mandate: &ValidatedMandate, calendar: &'c Wall) -> AccountRiskFold<'c> {
    AccountRiskFold::open(
        ACCOUNT,
        1,
        AGENT,
        mandate,
        &opening(),
        calendar,
        TripwireState::default(),
    )
    .unwrap()
}

/// A row on the account stream whose body is the event `payload` names.
fn row(seq: u64, event_type: &str, schema_version: u64, payload: &str) -> StoredEvent {
    let body = format!(
        r#"{{"event_type":"{event_type}","schema_version":{schema_version},"seq":{seq},
        "stream_id":"{ACCOUNT}","causation_id":"01J8Z4C0000000000000000000","payload":{payload}}}"#
    );
    stored(
        seq,
        event_type,
        schema_version,
        to_canonical(&parse(body.as_bytes()).unwrap()),
    )
}

fn stored(seq: u64, event_type: &str, schema_version: u64, body: Vec<u8>) -> StoredEvent {
    let hash = mandate_canon::Digest::of(&body);
    StoredEvent {
        stream_id: ACCOUNT.to_owned(),
        seq,
        event_id: format!("01J8Z4R00000000000000000{seq:02}"),
        event_type: event_type.to_owned(),
        schema_version,
        environment: "paper".to_owned(),
        recorded_at: OPENED.to_owned(),
        prev_hash: hash,
        hash,
        body,
    }
}

/// The executor's copy of a `ClockAdvanced` tick, which carries the scheduler's `clock`.
fn tick(seq: u64, clock: &str) -> StoredEvent {
    row(
        seq,
        "ClockAdvanced",
        1,
        &format!(r#"{{"clock":"{clock}"}}"#),
    )
}

fn readable_head(view: &RiskView) -> u64 {
    match view {
        RiskView::Readable { head, .. } => *head,
        other => panic!("the view is readable: {other:?}"),
    }
}

/// The cause and head of an unreadable view, after checking it holds openings and nothing else
/// at `exits_only`.
fn unreadable(view: &RiskView) -> (UnreadableCause, u64) {
    let RiskView::Unreadable { cause, head, mode } = view else {
        panic!("the view is unreadable: {view:?}");
    };
    assert_eq!(*mode, AgentMode::ExitsOnly, "from a readable `normal`");
    assert!(view.holds(OrderPath::Opening), "openings wait");
    for path in [
        OrderPath::OwnerExit,
        OrderPath::DiscretionaryExit,
        OrderPath::RiskExit,
        OrderPath::ProtectiveOrder,
        OrderPath::KillSwitch,
    ] {
        assert!(!view.holds(path), "{path:?} goes on (rule 13)");
    }
    (*cause, *head)
}

#[test]
fn the_opened_view_is_the_risk_states_opening_snapshot() {
    let mandate = mandate();
    let fold = open(&mandate, &Wall);
    let state = RiskState::open(&mandate, &opening(), &Wall).unwrap();
    assert_eq!(
        fold.view(),
        &RiskView::Readable {
            snapshot: Box::new(state.snapshot().clone()),
            head: 1,
        }
    );
    assert!(
        !fold.view().holds(OrderPath::Opening),
        "a readable `normal` view holds nothing"
    );
}

#[test]
fn a_row_on_another_stream_is_refused_and_changes_nothing() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    let before = fold.view().clone();
    let mut foreign = tick(2, "2026-09-22T14:30:01.000000000Z");
    foreign.stream_id = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000B2".to_owned();
    assert!(matches!(
        fold.fold(&foreign),
        Err(ExecutorError::ForeignStream { .. })
    ));
    assert_eq!(fold.view(), &before);
}

#[test]
fn a_failed_append_makes_the_view_unreadable_for_good() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    fold.append_failed();
    assert_eq!(unreadable(fold.view()), (UnreadableCause::AppendFailed, 1));
    fold.append_failed();
    assert_eq!(unreadable(fold.view()).0, UnreadableCause::AppendFailed);
}

/// Only `paused` or `stopped` holds an exit or a protective order, and no mode holds a kill switch
/// (`AGENTS.md` rule 13), readable or not.
#[test]
fn only_paused_or_stopped_holds_an_exit_and_nothing_holds_a_kill_switch() {
    let mandate = mandate();
    let opened = open(&mandate, &Wall).view().clone();
    for path in [
        OrderPath::OwnerExit,
        OrderPath::DiscretionaryExit,
        OrderPath::RiskExit,
        OrderPath::ProtectiveOrder,
        OrderPath::KillSwitch,
    ] {
        assert!(
            !opened.holds(path),
            "a readable `normal` view holds no {path:?}"
        );
    }
    let unread_normal = RiskView::Unreadable {
        cause: UnreadableCause::SeqGap,
        head: 1,
        mode: AgentMode::Normal,
    };
    assert!(
        unread_normal.holds(OrderPath::Opening),
        "being unreadable holds openings, whatever mode it carries"
    );
    for mode in [AgentMode::Paused, AgentMode::Stopped] {
        let view = RiskView::Unreadable {
            cause: UnreadableCause::SeqGap,
            head: 1,
            mode,
        };
        assert_eq!(view.mode(), mode);
        for path in [
            OrderPath::Opening,
            OrderPath::OwnerExit,
            OrderPath::DiscretionaryExit,
            OrderPath::RiskExit,
            OrderPath::ProtectiveOrder,
        ] {
            assert!(view.holds(path), "{mode:?} holds {path:?}");
        }
        assert!(
            !view.holds(OrderPath::KillSwitch),
            "{mode:?} never holds a kill switch"
        );
    }
}

#[test]
fn a_row_the_fold_does_not_read_advances_its_head() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    let intent = row(
        2,
        "IntentReceived",
        1,
        r#"{"intent_id":"01J8Z4I0000000000000000000"}"#,
    );
    assert_eq!(fold.fold(&intent).unwrap(), Vec::<RiskAppend>::new());
    assert_eq!(readable_head(fold.view()), 2);
    assert_eq!(
        fold.fold(&tick(3, "2026-09-22T14:30:01.000000000Z"))
            .unwrap(),
        vec![]
    );
    assert_eq!(readable_head(fold.view()), 3);
}

#[test]
fn a_seq_gap_makes_the_view_unreadable() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    assert_eq!(
        fold.fold(&tick(3, "2026-09-22T14:30:01.000000000Z"))
            .unwrap(),
        vec![]
    );
    assert_eq!(unreadable(fold.view()), (UnreadableCause::SeqGap, 1));
}

#[test]
fn an_unknown_schema_version_makes_the_view_unreadable() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    let later = row(
        2,
        "ClockAdvanced",
        9,
        r#"{"clock":"2026-09-22T14:30:01.000000000Z"}"#,
    );
    assert_eq!(fold.fold(&later).unwrap(), vec![]);
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::UnknownSchemaVersion, 1)
    );
}

#[test]
fn an_event_type_outside_the_catalogue_makes_the_view_unreadable() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    assert_eq!(fold.fold(&row(2, "LimitWaived", 1, "{}")).unwrap(), vec![]);
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::UnfoldedEventType, 1)
    );
}

#[test]
fn an_undecodable_row_makes_the_view_unreadable() {
    let mandate = mandate();
    for body in [b"not json".to_vec(), br#"{"payload":{}}"#.to_vec()] {
        let mut fold = open(&mandate, &Wall);
        assert_eq!(
            fold.fold(&stored(2, "ClockAdvanced", 1, body)).unwrap(),
            vec![]
        );
        assert_eq!(unreadable(fold.view()), (UnreadableCause::Undecodable, 1));
    }
}

#[test]
fn a_refusal_to_decide_makes_the_view_unreadable() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    assert_eq!(
        fold.fold(&tick(2, "2026-09-22T14:30:05.000000000Z"))
            .unwrap(),
        vec![]
    );
    assert_eq!(
        fold.fold(&tick(3, "2026-09-22T14:30:04.000000000Z"))
            .unwrap(),
        vec![]
    );
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 2)
    );
}

#[test]
fn an_unreadable_view_stays_unreadable_and_appends_nothing() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    assert_eq!(fold.fold(&row(2, "LimitWaived", 1, "{}")).unwrap(), vec![]);
    assert_eq!(
        fold.fold(&tick(3, "2026-09-23T04:00:00.000000000Z"))
            .unwrap(),
        vec![],
        "not even the risk day a readable view would start"
    );
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::UnfoldedEventType, 1)
    );
}

#[test]
fn a_tick_across_new_york_midnight_appends_the_risk_day_the_journal_accepts() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    let appends = fold
        .fold(&tick(2, "2026-09-23T04:00:00.000000000Z"))
        .unwrap();
    let [append] = appends.as_slice() else {
        panic!("one record: {appends:?}");
    };
    assert_eq!(
        (append.event_type, append.schema_version),
        ("RiskDayStarted", 1)
    );
    assert_eq!(
        append.causation_id.as_deref(),
        Some("01J8Z4R0000000000000000002"),
        "it follows from the tick"
    );
    let text = |member: &str| append.payload.get(member).and_then(Value::as_str);
    assert_eq!(text("agent_id"), Some(AGENT));
    assert_eq!(text("day"), Some("2026-09-23"), "not {OPENED_DAY}");
    assert_eq!(
        text("day_start_equity"),
        Some("10000"),
        "E at the allocation"
    );
    assert_eq!(text("risk_clock"), Some("2026-09-23T04:00:00.000000000Z"));
    assert_eq!(readable_head(fold.view()), 2);
    let fixture = std::fs::read("../../fixtures/refcases/journal.json").unwrap();
    let section = parse(&fixture).unwrap();
    let mut draft = section
        .get("risk_limits")
        .and_then(|s| s.get("drafts"))
        .and_then(|d| d.get("day_started"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    draft.insert(Key::new("payload").unwrap(), append.payload.clone());
    let parsed = Draft::parse(&to_canonical(&Value::Object(draft)));
    assert!(parsed.is_ok(), "§9.27 accepts it: {:?}", parsed.err());
}

/// Every second counts and every tick is crypto's, as the harness's crypto cases step (§5.2).
struct Crypto;

impl SessionClock for Crypto {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        Wall.seconds_between(from, to)
    }
}

impl RiskCalendar for Crypto {
    fn session(&self, _at: UtcNanos) -> Result<MarketSession, SpecError> {
        Ok(MarketSession::Crypto)
    }
}

/// An equity's clock: the regular-session seconds of `mandate-time`'s NYSE calendar, which the
/// harness counts staleness on (DEC-2305 item 8). Every tick these walks fold is in the regular
/// session; a mark's session is its record's (DEC-2305 item 1).
struct Nyse(ExchangeCalendar);

impl SessionClock for Nyse {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        let mut total = 0;
        let mut date = from.date();
        while date <= to.date() {
            for span in self.0.sessions(date)? {
                let (start, end) = (span.start().max(from), span.end().min(to));
                if span.session() == Session::Regular && start < end {
                    total += Wall.seconds_between(start, end)?;
                }
            }
            date = date.next()?;
        }
        Ok(total)
    }
}

impl RiskCalendar for Nyse {
    fn session(&self, _at: UtcNanos) -> Result<MarketSession, SpecError> {
        Ok(MarketSession::Regular)
    }
}

fn nyse() -> Nyse {
    Nyse(ExchangeCalendar::us_equities().unwrap())
}

fn case(id: &str) -> Json {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().unwrap();
    cases.iter().find(|case| case["id"] == id).unwrap().clone()
}

/// A case's patched mandate.
fn case_mandate(case: &Json) -> ValidatedMandate {
    validated(
        case["base"].as_str().unwrap(),
        case["patch"].as_array().unwrap(),
    )
}

/// The fold opened at a case's `initial`, with `harness_defaults`' staleness limit.
fn open_case<'c>(
    case: &Json,
    mandate: &ValidatedMandate,
    calendar: &'c dyn RiskCalendar,
) -> AccountRiskFold<'c> {
    let initial = |key: &str| case["initial"][key].as_str().unwrap().to_owned();
    let max_age = fixture()["harness_defaults"]["mark_max_age_s"].as_u64();
    let opening = Opening {
        position_qty: Qty::parse(&initial("position_qty")).unwrap(),
        avg_cost: Price::parse(&initial("avg_cost")).unwrap(),
        asset_class: AssetClass::parse(&initial("asset_class")).unwrap(),
        at: at(&initial("at")),
        inherited_loss_usd: Usd::parse(&initial("inherited_loss_usd")).unwrap(),
        mark_max_age_s: u32::try_from(max_age.unwrap()).unwrap(),
    };
    let tripwires = TripwireState::default();
    AccountRiskFold::open(ACCOUNT, 1, AGENT, mandate, &opening, calendar, tripwires).unwrap()
}

/// A version-2 `MarkUpdated` for `instrument` in §9.30's member order, its `price` the bid.
fn mark(
    seq: u64,
    instrument: &str,
    clock: &str,
    bid: &str,
    sane: bool,
    session: &str,
) -> StoredEvent {
    row(
        seq,
        "MarkUpdated",
        2,
        &format!(
            r#"{{"instrument_id":"{instrument}","price":"{bid}","source":"quote","feed":"iex",
            "bid":"{bid}","sane":{sane},"session":"{session}","risk_clock":"{clock}"}}"#
        ),
    )
}

/// The instrument a case's marks quote: the reference cases name none, so BTC/USD for crypto and
/// AAPL for an equity.
fn instrument(case: &Json) -> &'static str {
    match case["initial"]["asset_class"].as_str().unwrap() {
        "crypto" => "BTC/USD",
        _ => "AAPL",
    }
}

/// The §9.27 records, whose members a step's journal states.
const CLOSED_RECORDS: [&str; 7] = [
    "RiskDayStarted",
    "RiskLimitTriggered",
    "RiskLimitLifted",
    "HighWaterMarkReset",
    "PositionReleased",
    "InstrumentRestrictionChanged",
    "GoalCompleted",
];

fn mode_name(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Normal => "normal",
        AgentMode::ExitsOnly => "exits_only",
        AgentMode::Paused => "paused",
        AgentMode::Stopped => "stopped",
    }
}

/// The view's snapshot against a step's `expect`: the reported figures, the mode, and both name
/// sets, as the harness compares them.
fn check_snapshot(view: &RiskView, seq: u64, expect: &Json) {
    let RiskView::Readable { snapshot, head } = view else {
        panic!("seq {seq} leaves the view readable: {view:?}");
    };
    assert_eq!(*head, seq);
    for (member, actual) in [
        ("agent_equity", snapshot.agent_equity.to_string()),
        ("high_water_mark", snapshot.high_water_mark.to_string()),
        ("drawdown", snapshot.drawdown.to_string()),
        ("day_start_equity", snapshot.day_start_equity.to_string()),
        ("daily_pnl", snapshot.daily_pnl.to_string()),
        (
            "daily_pnl_fraction",
            snapshot.daily_pnl_fraction.to_string(),
        ),
        ("capital_base", snapshot.capital_base.to_string()),
        ("size_factor", snapshot.size_factor.to_string()),
        ("net_contributed", snapshot.net_contributed.to_string()),
        ("agent_mode", mode_name(snapshot.agent_mode).to_owned()),
    ] {
        assert_eq!(actual, expect[member], "seq {seq}: {member}");
    }
    let names = |key: &str| -> BTreeSet<String> {
        let listed = expect[key].as_array().unwrap().iter();
        listed
            .map(|name| name.as_str().unwrap().to_owned())
            .collect()
    };
    let held = snapshot.restrictions.iter().map(|r| r.as_str().to_owned());
    assert_eq!(held.collect::<BTreeSet<_>>(), names("restrictions"));
    let instrument = snapshot.instrument_restrictions.iter();
    let instrument = instrument
        .map(|r| r.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(instrument, names("instrument_restrictions"));
}

/// The appended records against a step's `journal`: every type in §5.2's order, and every member
/// the step states of a §9.27 record.
fn check_records(appends: &[RiskAppend], seq: u64, journal: &Json) {
    let journal = journal.as_array().unwrap();
    let types: Vec<&str> = appends.iter().map(|append| append.event_type).collect();
    let wanted: Vec<&str> = journal
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, wanted, "seq {seq}: the records, in order");
    for (append, entry) in appends.iter().zip(journal) {
        if !CLOSED_RECORDS.contains(&append.event_type) {
            continue;
        }
        for (member, value) in entry.as_object().unwrap() {
            let actual = append.payload.get(member);
            let same = match value {
                Json::String(text) if member == "type" => text == append.event_type,
                Json::String(text) => actual.and_then(Value::as_str) == Some(text.as_str()),
                Json::Bool(flag) => actual == Some(&Value::Bool(*flag)),
                other => panic!("seq {seq}: a member the walk does not compare: {other}"),
            };
            assert!(same, "seq {seq}: {member} is {value}, not {actual:?}");
        }
    }
}

/// Folds a case's steps as account-stream rows from seq 2, a `mark` as a version-2 `MarkUpdated`
/// and a `clock` or `risk_day_started` as a tick, and checks each against its `expect`.
fn walk(fold: &mut AccountRiskFold<'_>, instrument: &str, steps: &[Json]) {
    for (seq, step) in (2..).zip(steps) {
        let text = |key: &str| step[key].as_str().unwrap();
        let row = match text("event") {
            "mark" => mark(
                seq,
                instrument,
                text("at"),
                text("bid"),
                step["sane"].as_bool().unwrap(),
                text("session"),
            ),
            "clock" | "risk_day_started" => tick(seq, text("at")),
            other => panic!("not a mark or a tick: {other}"),
        };
        let appends = fold.fold(&row).unwrap();
        check_records(&appends, seq, &step["expect"]["journal"]);
        check_snapshot(fold.view(), seq, &step["expect"]);
    }
}

fn readable_in(mode: AgentMode) -> RiskView {
    let mandate = mandate();
    let state = RiskState::open(&mandate, &opening(), &Wall).unwrap();
    let mut snapshot = state.snapshot().clone();
    snapshot.agent_mode = mode;
    RiskView::Readable {
        snapshot: Box::new(snapshot),
        head: 1,
    }
}

const EXITS: [OrderPath; 4] = [
    OrderPath::OwnerExit,
    OrderPath::DiscretionaryExit,
    OrderPath::RiskExit,
    OrderPath::ProtectiveOrder,
];

#[test]
fn a_readable_exits_only_view_holds_openings_only() {
    let view = readable_in(AgentMode::ExitsOnly);
    assert!(view.holds(OrderPath::Opening));
    for path in EXITS.into_iter().chain([OrderPath::KillSwitch]) {
        assert!(!view.holds(path), "exits_only holds no {path:?} (rule 13)");
    }
}

#[test]
fn a_readable_paused_or_stopped_view_holds_every_exit_but_never_the_kill_switch() {
    for mode in [AgentMode::Paused, AgentMode::Stopped] {
        let view = readable_in(mode);
        for path in EXITS.into_iter().chain([OrderPath::Opening]) {
            assert!(view.holds(path), "{mode:?} holds {path:?}");
        }
        assert!(!view.holds(OrderPath::KillSwitch), "{mode:?}");
    }
}

#[test]
fn an_unreadable_view_from_a_readable_normal_is_exits_only() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    assert_eq!(fold.view().mode(), AgentMode::Normal);
    assert_eq!(fold.fold(&row(2, "LimitWaived", 1, "{}")).unwrap(), vec![]);
    assert_eq!(fold.view().mode(), AgentMode::ExitsOnly);
}

/// MC-R20's opening holds 0.1 BTC at 60000 on an allocation of 10000, so a mark at 1 read as a
/// sane price would take E to 4000.1.
#[test]
fn a_version_1_mark_is_no_verdict_and_moves_nothing() {
    let case = case("MC-R20");
    let mandate = case_mandate(&case);
    let mut fold = open_case(&case, &mandate, &Crypto);
    let v1 = row(
        2,
        "MarkUpdated",
        1,
        r#"{"instrument_id":"BTC/USD","price":"1","source":"quote","feed":"iex",
        "risk_clock":"2026-09-21T04:00:30.000000000Z"}"#,
    );
    assert_eq!(fold.fold(&v1).unwrap(), vec![]);
    let RiskView::Readable { snapshot, head } = fold.view() else {
        panic!("a version-1 mark is readable: {:?}", fold.view());
    };
    assert_eq!(
        (snapshot.agent_equity.to_string(), *head),
        ("10000".into(), 2)
    );
    assert!(snapshot.instrument_restrictions.is_empty(), "30 s is fresh");
}

/// `RiskState` holds one position, so a sane BTC/USD mark at 57990 moves MC-R20's E to
/// 10000 + 0.1 × (57990 − 60000) = 9799, and a mark naming a second instrument is one the fold
/// cannot step without pricing BTC's position at it (DEC-2308 item 4): the view becomes
/// unreadable, holding openings and no exit.
#[test]
fn a_mark_for_a_second_instrument_makes_the_view_unreadable() {
    let case = case("MC-R20");
    let mandate = case_mandate(&case);
    let mut fold = open_case(&case, &mandate, &Crypto);
    let first = mark(
        2,
        "BTC/USD",
        "2026-09-21T04:00:30.000000000Z",
        "57990",
        true,
        "crypto",
    );
    assert_eq!(fold.fold(&first).unwrap(), vec![]);
    let RiskView::Readable { snapshot, .. } = fold.view() else {
        panic!("the first instrument's mark is readable: {:?}", fold.view());
    };
    assert_eq!(snapshot.agent_equity.to_string(), "9799");
    let other = mark(
        3,
        "ETH/USD",
        "2026-09-21T04:01:00.000000000Z",
        "1",
        true,
        "crypto",
    );
    assert_eq!(fold.fold(&other).unwrap(), vec![]);
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 2)
    );
    assert_eq!(fold.view().mode(), AgentMode::ExitsOnly);
    assert!(fold.view().holds(OrderPath::Opening));
    for path in EXITS.into_iter().chain([OrderPath::KillSwitch]) {
        assert!(!fold.view().holds(path), "{path:?} is never held (rule 13)");
    }
}

/// §5.2 never reads a not-sane mark's bid, so the same not-sane mark on a flat position with two
/// different bids leaves two equal views.
#[test]
fn a_not_sane_marks_step_ignores_its_bid() {
    let mandate = mandate();
    let views: Vec<RiskView> = ["1", "99999"]
        .into_iter()
        .map(|bid| {
            let mut fold = open(&mandate, &Wall);
            let row = mark(
                2,
                "AAPL",
                "2026-09-22T14:30:01.000000000Z",
                bid,
                false,
                "regular",
            );
            assert_eq!(fold.fold(&row).unwrap(), vec![], "bid {bid}");
            fold.view().clone()
        })
        .collect();
    assert!(
        matches!(views[0], RiskView::Readable { head: 2, .. }),
        "{:?}",
        views[0]
    );
    assert_eq!(views[0], views[1]);
}

/// MC-R20 opens at New York midnight on 2026-09-21 (04:00Z). A sane mark at 04:00:30Z the next day,
/// with no tick between, starts no risk day: only a tick does (§5.4, DEC-2308 item 6).
#[test]
fn a_mark_past_new_york_midnight_with_no_tick_starts_no_risk_day() {
    let case = case("MC-R20");
    let mandate = case_mandate(&case);
    let mut fold = open_case(&case, &mandate, &Crypto);
    let late = mark(
        2,
        "BTC/USD",
        "2026-09-22T04:00:30.000000000Z",
        "57990",
        true,
        "crypto",
    );
    assert_eq!(fold.fold(&late).unwrap(), vec![]);
    let RiskView::Readable { snapshot, head } = fold.view() else {
        panic!("the mark is readable: {:?}", fold.view());
    };
    assert_eq!(
        (snapshot.agent_equity.to_string(), *head),
        ("9799".into(), 2)
    );
}

#[test]
fn a_mark_after_version_2_is_an_unknown_schema_version() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    let mut later = mark(
        2,
        "AAPL",
        "2026-09-22T14:30:01.000000000Z",
        "1",
        true,
        "regular",
    );
    later.schema_version = 3;
    assert_eq!(fold.fold(&later).unwrap(), vec![]);
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::UnknownSchemaVersion, 1)
    );
}

/// MC-R20: sane crypto marks move E at any hour, and the tick at New York midnight starts the risk
/// day from E (§5.4). Its first step, the risk day at the opening instant, is the opened state.
#[test]
fn mc_r20_crypto_marks_and_ticks_fold_as_the_reference_case() {
    let case = case("MC-R20");
    let mandate = case_mandate(&case);
    let mut fold = open_case(&case, &mandate, &Crypto);
    let steps = case["steps"].as_array().unwrap();
    check_snapshot(fold.view(), 1, &steps[0]["expect"]);
    walk(&mut fold, instrument(&case), &steps[1..]);
}

/// MC-R13's first three steps: two sane regular-session marks at 100, then a sane pre-market mark
/// at 80 the next morning, which leaves the equity's E at 10000 since only a regular-session mark
/// updates it (§5.2). Its later steps, the not-sane mark and the staleness tick, journal
/// `InstrumentRestrictionChanged`, which B2b-2 writes. The case has no after-hours or overnight
/// mark, a gap the E6-15 backlog entry records.
#[test]
fn mc_r13_session_marks_leave_an_equitys_equity_as_the_reference_case() {
    let case = case("MC-R13");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let mut fold = open_case(&case, &mandate, &calendar);
    let steps = case["steps"].as_array().unwrap();
    walk(&mut fold, instrument(&case), &steps[..3]);
}
