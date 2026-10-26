//! E6-15 slice B1 (DEC-2301 item 3, DEC-2304): `risk_state::AccountRiskFold` reads the account
//! stream's rows into a risk view, and a row it cannot read makes the view unreadable for good. An
//! unreadable view holds new openings and no exit, protective order, risk exit, or kill switch
//! (`AGENTS.md` rules 3 and 13). Until B2 fills in `fold`, every test that folds a row fails at its
//! `Unimplemented` stub.
//!
//! E6-15 slice B2b-1 (DEC-2305, journal spec §9.30): a version-2 `MarkUpdated` steps the risk state
//! with its verdict, a version-1 mark stays no verdict, and the reference cases' marks and ticks,
//! driven as account-stream rows, give the harness's snapshots and records (DEC-2304 item 6).
//!
//! E6-15 slice B2b-2a (journal spec §9.27, DEC-2307 items 3 and 4): every record the risk state
//! journals is appended with its closed payload, `AgentModeApplied` among them and the kill switch
//! never, and `open` refuses an instrument that is not a canonical asset id (DEC-2313), so every
//! instrument here is one. Until B2b-2 lands, each test that reaches such a record or refusal fails
//! at its `Unimplemented` stub.

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
        None,
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

/// A close request of the account kill switch (journal spec v0.75 §9.5, DEC-2337) is a record the
/// risk fold reads and acts on nowhere: the view stays readable with its snapshot unchanged, only
/// its head advances, and nothing is appended.
#[test]
fn a_close_request_leaves_the_risk_view_readable_and_unchanged() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    let RiskView::Readable {
        snapshot: before, ..
    } = fold.view().clone()
    else {
        panic!("the opened view is readable");
    };
    let close = row(
        2,
        "AccountCloseRequested",
        1,
        r#"{"instrument_id":"AAPL","qty":"10","kill_switch_event":"01J8Z4C0000000000000000000",
        "attempt":1,"risk_clock":"2026-09-22T14:30:01.000000000Z"}"#,
    );

    assert_eq!(fold.fold(&close).unwrap(), Vec::<RiskAppend>::new());
    match fold.view() {
        RiskView::Readable { snapshot, head } => {
            assert_eq!(
                (snapshot, *head),
                (&before, 2),
                "unchanged, at the close's seq"
            );
        }
        other => panic!("the view stays readable: {other:?}"),
    }
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
    opened(case, mandate, calendar, instrument(case)).unwrap()
}

/// [`open_case`] on the position's instrument `held`, or why `open` refused it.
fn opened<'c>(
    case: &Json,
    mandate: &ValidatedMandate,
    calendar: &'c dyn RiskCalendar,
    held: &str,
) -> Result<AccountRiskFold<'c>, SpecError> {
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
    AccountRiskFold::open(
        ACCOUNT,
        1,
        AGENT,
        mandate,
        &opening,
        Some(held),
        calendar,
        tripwires,
    )
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

/// `btc_accumulator`'s pinned BTC/USD, by its canonical asset ID (DEC-2313 item 1).
const BTC: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";

/// `two_stock_swing`'s and `research_equity_pinned`'s pinned XYZ, by its canonical asset ID.
const XYZ: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";

/// A second instrument no reference base pins, by a canonical asset ID: ETH/USD where a test names
/// one.
const ETH: &str = "7b4a1c2e-4444-4a2b-9c3d-000000000004";

/// The instrument a case's position is and its marks quote: the reference cases name none, so the
/// bases' pinned BTC/USD for crypto and XYZ for an equity.
fn instrument(case: &Json) -> &'static str {
    match case["initial"]["asset_class"].as_str().unwrap() {
        "crypto" => BTC,
        _ => XYZ,
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

/// The snapshot members §9.27 gives `RiskLimitTriggered` and `RiskLimitLifted` that a step's
/// `expect` states.
const SNAPSHOT_MEMBERS: [&str; 6] = [
    "agent_equity",
    "high_water_mark",
    "drawdown",
    "day_start_equity",
    "capital_base",
    "net_contributed",
];

/// `risk_limits`' base `RiskDayStarted` draft from the journal's test vectors, whose envelope every
/// risk record the walks append is checked in.
fn base_draft() -> Json {
    let vectors = std::fs::read("../../fixtures/refcases/journal.json").unwrap();
    let vectors: Json = serde_json::from_slice(&vectors).unwrap();
    vectors["risk_limits"]["drafts"]["day_started"].clone()
}

/// Whether the journal accepts `append` on the account stream: §9.27 closes the risk records, and
/// §9.6 closes `AgentModeApplied`, which names no configuration.
fn accepted(base: &Json, append: &RiskAppend) -> bool {
    let mut draft = base.clone();
    draft["event_type"] = Json::from(append.event_type);
    draft["schema_version"] = Json::from(append.schema_version);
    draft["payload"] = serde_json::from_slice(&to_canonical(&append.payload)).unwrap();
    if append.event_type == "AgentModeApplied" {
        draft["config_refs"] = Json::Object(serde_json::Map::new());
    }
    let canonical = parse(&serde_json::to_vec(&draft).unwrap()).unwrap();
    Draft::parse(&to_canonical(&canonical)).is_ok()
}

/// The appended records against a step: every type in §5.2's order but `KillSwitchActivated`, which
/// the fold never runs (DEC-2307 item 3), and every member the step states but `AgentModeApplied`'s
/// `from`, which §9.6's closed payload does not carry. Each record follows from its row at the row's
/// clock for this agent and is one the journal accepts; a trigger or a lift carries the step's
/// snapshot, `AgentModeApplied` is the executor's own and names the action of the step's last trigger
/// before it as its restriction (DEC-2308 item 3), and an instrument restriction names the
/// instrument the marks named (DEC-2307 item 4, DEC-2308 item 2).
fn check_records(appends: &[RiskAppend], row: &StoredEvent, instrument: &str, step: &Json) {
    let (seq, base) = (row.seq, base_draft());
    let journal = step["expect"]["journal"].as_array().unwrap().iter();
    let journal: Vec<&Json> = journal
        .filter(|e| e["type"] != "KillSwitchActivated")
        .collect();
    let types: Vec<&str> = appends.iter().map(|append| append.event_type).collect();
    let wanted: Vec<&str> = journal
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, wanted, "seq {seq}: the records, in order");
    let mut action = None;
    for (append, entry) in appends.iter().zip(journal) {
        let mode = append.event_type == "AgentModeApplied";
        if entry["type"] == "RiskLimitTriggered" {
            action = entry["action"].as_str();
        }
        if !mode && !CLOSED_RECORDS.contains(&append.event_type) {
            continue;
        }
        let text = |member: &str| append.payload.get(member).and_then(Value::as_str);
        assert_eq!(append.causation_id.as_deref(), Some(row.event_id.as_str()));
        assert_eq!(text("risk_clock"), step["at"].as_str(), "seq {seq}");
        assert_eq!(text(if mode { "agent" } else { "agent_id" }), Some(AGENT));
        assert!(accepted(&base, append), "seq {seq}: {append:?}");
        match append.event_type {
            "RiskLimitTriggered" | "RiskLimitLifted" => {
                for member in SNAPSHOT_MEMBERS {
                    let wanted = step["expect"][member].as_str();
                    assert_eq!(text(member), wanted, "seq {seq}: {member}");
                }
            }
            "InstrumentRestrictionChanged" => assert_eq!(text("instrument"), Some(instrument)),
            "AgentModeApplied" => {
                assert_eq!(append.payload.get("originated"), Some(&Value::Bool(true)));
                assert_eq!(text("restriction"), action, "seq {seq}: the rung's action");
            }
            _ => {}
        }
        for (member, value) in entry.as_object().unwrap() {
            let actual = append.payload.get(member);
            let same = match value {
                _ if mode && member == "from" => true,
                Json::String(text) if member == "type" => text == append.event_type,
                Json::String(text) => actual.and_then(Value::as_str) == Some(text.as_str()),
                Json::Bool(flag) => actual == Some(&Value::Bool(*flag)),
                other => panic!("seq {seq}: a member the walk does not compare: {other}"),
            };
            assert!(same, "seq {seq}: {member} is {value}, not {actual:?}");
        }
    }
}

/// Folds a case's steps as account-stream rows from seq 2, a `mark` as a version-2 `MarkUpdated`,
/// a `clock` or `risk_day_started` as a tick, and a `fill` as the agent's own order (its request and
/// submission, which step nothing) and then its `FillApplied` (DEC-2393 items 1 and 2), checks each
/// stepping row against its `expect`, and returns each step's records.
fn walk(fold: &mut AccountRiskFold<'_>, instrument: &str, steps: &[Json]) -> Vec<Vec<RiskAppend>> {
    walk_from(fold, instrument, 2, steps)
}

/// [`walk`] from the row at `first`.
fn walk_from(
    fold: &mut AccountRiskFold<'_>,
    instrument: &str,
    first: u64,
    steps: &[Json],
) -> Vec<Vec<RiskAppend>> {
    let mut records = Vec::new();
    let mut seq = first;
    for step in steps {
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
            "fill" => {
                let order = format!("o-{seq}");
                for row in own_order(seq, AGENT, &order, instrument, text("side"), text("qty")) {
                    assert_eq!(
                        fold.fold(&row).unwrap(),
                        Vec::new(),
                        "an order steps nothing"
                    );
                }
                seq += 2;
                fill(
                    seq,
                    &order,
                    instrument,
                    (text("side"), text("qty"), text("price")),
                    text("at"),
                    "[]",
                )
            }
            other => panic!("not a mark, a tick or a fill: {other}"),
        };
        let appends = fold.fold(&row).unwrap();
        check_records(&appends, &row, instrument, step);
        check_snapshot(fold.view(), seq, &step["expect"]);
        records.push(appends);
        seq += 1;
    }
    records
}

/// A row on the account stream with its own `causation_id`.
fn caused(
    seq: u64,
    event_type: &str,
    version: u64,
    cause: Option<&str>,
    payload: &str,
) -> StoredEvent {
    let cause = cause.map_or("null".to_owned(), |cause| format!(r#""{cause}""#));
    let body = format!(
        r#"{{"event_type":"{event_type}","schema_version":{version},"seq":{seq},
        "stream_id":"{ACCOUNT}","causation_id":{cause},"payload":{payload}}}"#
    );
    stored(
        seq,
        event_type,
        version,
        to_canonical(&parse(body.as_bytes()).unwrap()),
    )
}

/// `agent`'s order `order` at seqs `seq` and `seq + 1`: its `OrderRequestRecorded`, then the
/// `OrderSubmitted` at version 2 whose `causation_id` names it (journal spec rule 45).
fn own_order(
    seq: u64,
    agent: &str,
    order: &str,
    instrument: &str,
    side: &str,
    qty: &str,
) -> [StoredEvent; 2] {
    let request = caused(
        seq,
        "OrderRequestRecorded",
        1,
        None,
        &format!(
            r#"{{"agent_id":"{agent}","intent_id":null,"purpose":"open","extended_hours":false,
            "stop_price":null,"order_class":null,"take_profit":null,"stop":null,"rung":null,
            "at_floor":null,"risk_clock":"{OPENED}"}}"#
        ),
    );
    let submitted = caused(
        seq + 1,
        "OrderSubmitted",
        2,
        Some(&request.event_id),
        &format!(
            r#"{{"client_order_id":"{order}","attempt":1,"instrument_id":"{instrument}",
            "side":"{side}","type":"limit","tif":"day","qty":"{qty}","limit_price":"100",
            "risk_clock":"{OPENED}"}}"#
        ),
    );
    [request, submitted]
}

/// A `FillApplied` of `order`: `(side, qty, price)` at `clock`, with the `fees` list `fees`.
fn fill(
    seq: u64,
    order: &str,
    instrument: &str,
    trade: (&str, &str, &str),
    clock: &str,
    fees: &str,
) -> StoredEvent {
    let (side, qty, price) = trade;
    row(
        seq,
        "FillApplied",
        1,
        &format!(
            r#"{{"fill_id":"f-{seq}","client_order_id":"{order}","instrument_id":"{instrument}",
            "side":"{side}","qty_gross":"{qty}","price":"{price}","trade_date":"{OPENED_DAY}",
            "risk_clock":"{clock}","fees":{fees}}}"#
        ),
    )
}

/// A `FeesCharged` the paper executor books for `order`, charging `charged`.
fn fees_charged(seq: u64, order: &str, charged: &str, clock: &str) -> StoredEvent {
    row(
        seq,
        "FeesCharged",
        1,
        &format!(
            r#"{{"family":"equities","day":"{OPENED_DAY}","accrued":"{charged}","charged":"{charged}",
            "simulated":true,"client_order_id":"{order}","sec":null,"taf":null,"cat":"{charged}",
            "risk_clock":"{clock}"}}"#
        ),
    )
}

fn equity(view: &RiskView) -> String {
    match view {
        RiskView::Readable { snapshot, .. } => snapshot.agent_equity.to_string(),
        other => panic!("the view is readable: {other:?}"),
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
        r#"{"instrument_id":"7b4a1c2e-1111-4a2b-9c3d-000000000001","price":"1","source":"quote","feed":"iex",
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
        BTC,
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
        ETH,
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

/// MC-R20 opens holding BTC/USD, bound at open, so a first mark naming ETH/USD at 59000 is never
/// read as BTC's price (which would make E 9900): the view becomes unreadable (DEC-2310 item 1).
#[test]
fn a_held_openings_first_mark_for_another_instrument_makes_the_view_unreadable() {
    let case = case("MC-R20");
    let mandate = case_mandate(&case);
    let mut fold = open_case(&case, &mandate, &Crypto);
    let other = mark(
        2,
        ETH,
        "2026-09-21T04:00:30.000000000Z",
        "59000",
        true,
        "crypto",
    );
    assert_eq!(fold.fold(&other).unwrap(), vec![]);
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 1)
    );
    assert_eq!(fold.view().mode(), AgentMode::ExitsOnly);
    assert!(fold.view().holds(OrderPath::Opening));
    for path in EXITS.into_iter().chain([OrderPath::KillSwitch]) {
        assert!(!fold.view().holds(path), "{path:?} is never held (rule 13)");
    }
}

/// A held opening, or a fold opened past seq 1, must name its instrument: the first could otherwise
/// be bound to whatever a mark names, and the second has lost the binding an earlier row made.
#[test]
fn an_opening_that_cannot_bind_its_instrument_is_refused() {
    let mandate = mandate();
    let held = Opening {
        position_qty: Qty::parse("1").unwrap(),
        ..opening()
    };
    let refused = |head: u64, opening: &Opening| {
        AccountRiskFold::open(
            ACCOUNT,
            head,
            AGENT,
            &mandate,
            opening,
            None,
            &Wall,
            TripwireState::default(),
        )
        .err()
    };
    let want = Some(SpecError::InvalidInput { what: "instrument" });
    assert_eq!(refused(1, &held), want, "a held opening");
    assert_eq!(refused(6, &opening()), want, "a flat opening past seq 1");
    assert_eq!(refused(1, &opening()), None, "a flat opening at seq 1");
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
                XYZ,
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
        BTC,
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
        XYZ,
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

/// MC-R13 in full: after its session marks, a mark that is not sane sets `stale_mark` at once, a sane
/// one clears it, and the tick 120 regular-session seconds later sets it again (§5.2), each record
/// naming the marks' `instrument_id`, the tick's included (DEC-2308 item 2).
///
/// The case steps no `risk_day_started` between its two days, while the account stream ticks at New
/// York midnight and the fold starts the day at that tick (§5.4, DEC-2308 item 1). So the walk folds
/// that tick between steps 2 and 3: E₀ becomes step 2's E, 10000, the E₀ every later step expects,
/// and nothing else moves (30 regular-session seconds have passed since the last sane mark).
#[test]
fn mc_r13_session_and_insane_marks_fold_as_the_reference_case() {
    let case = case("MC-R13");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let mut fold = open_case(&case, &mandate, &calendar);
    let steps = case["steps"].as_array().unwrap();
    walk(&mut fold, XYZ, &steps[..2]);
    let midnight = fold.fold(&tick(4, "2026-09-22T04:00:00.000000000Z"));
    let midnight = midnight.unwrap();
    let [day] = midnight.as_slice() else {
        panic!("one record: {midnight:?}");
    };
    let text = |member: &str| day.payload.get(member).and_then(Value::as_str);
    assert_eq!(day.event_type, "RiskDayStarted");
    assert_eq!(
        text("day_start_equity"),
        steps[1]["expect"]["agent_equity"].as_str()
    );
    walk_from(&mut fold, XYZ, 5, &steps[2..]);
}

/// MC-R01's ladder reaches `paused`, and a row the fold cannot read after it keeps `paused`, which
/// holds every exit and protective order and never the kill switch (DEC-2304 item 3).
#[test]
fn an_unreadable_view_after_mc_r01s_paused_stays_paused() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let mut fold = open_case(&case, &mandate, &calendar);
    walk(&mut fold, XYZ, case["steps"].as_array().unwrap());
    let head = readable_head(fold.view());
    assert_eq!(fold.view().mode(), AgentMode::Paused);
    let waived = row(head + 1, "LimitWaived", 1, "{}");
    assert_eq!(fold.fold(&waived).unwrap(), vec![]);
    let paused = RiskView::Unreadable {
        cause: UnreadableCause::UnfoldedEventType,
        head,
        mode: AgentMode::Paused,
    };
    assert_eq!(fold.view(), &paused);
    for path in EXITS {
        assert!(paused.holds(path), "paused holds {path:?}");
    }
    assert!(!paused.holds(OrderPath::KillSwitch));
}

/// MC-R01's last mark crosses the `flatten_and_pause` rung: the fold returns the trigger, then
/// `AgentModeApplied` to `paused` in §9.6's closed payload, and nothing that acts, since the kill
/// switch the case journals is slice C5's action (DEC-2307 item 3). Every trigger of the walk
/// carries a breach time of 0 s: `breach_confirm_s` is 0, a scale rung has no confirmation, and no
/// rung is breached at the start of the interval before the mark that triggers it (§5.2, §5.6).
#[test]
fn a_flatten_and_pause_rung_returns_agent_mode_applied_and_nothing_that_acts() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let mut fold = open_case(&case, &mandate, &calendar);
    let records = walk(&mut fold, XYZ, case["steps"].as_array().unwrap());
    let last = records.last().unwrap();
    let types: Vec<&str> = last.iter().map(|append| append.event_type).collect();
    assert_eq!(types, ["RiskLimitTriggered", "AgentModeApplied"]);
    let text = |member: &str| last[0].payload.get(member).and_then(Value::as_str);
    assert_eq!(text("limit"), Some("drawdown_ladder[2]"));
    assert_eq!(text("action"), Some("flatten_and_pause"));
    let applied = last[1].payload.as_object().unwrap();
    let members: Vec<&str> = applied.keys().map(Key::as_str).collect();
    assert_eq!(
        members,
        ["agent", "originated", "restriction", "risk_clock", "to"]
    );
    let member = |name: &str| applied[&Key::new(name).unwrap()].as_str();
    assert_eq!(member("to"), Some("paused"));
    assert_eq!(
        member("restriction"),
        Some("flatten_and_pause"),
        "DEC-2308 item 3"
    );
    let triggers = records.iter().flatten();
    for trigger in triggers.filter(|append| append.event_type == "RiskLimitTriggered") {
        let breach = trigger.payload.get("breach_time_s").map(to_canonical);
        assert_eq!(breach, Some(b"0".to_vec()), "{trigger:?}");
    }
    assert!(fold.view().holds(OrderPath::RiskExit), "paused holds exits");
    assert!(!fold.view().holds(OrderPath::KillSwitch));
}

/// DEC-2307 item 4: an instrument restriction names the mark's `instrument_id` as written. MC-R13
/// opens holding 100 shares, so a mark that is not sane sets `stale_mark` at once (§5.2), and the
/// one record is this, member for member.
#[test]
fn an_instrument_restriction_names_the_marks_instrument_as_written() {
    let case = case("MC-R13");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let mut fold = open_case(&case, &mandate, &calendar);
    let clock = "2026-09-21T13:32:00.000000000Z";
    let insane = mark(2, XYZ, clock, "99", false, "regular");
    let payload = format!(
        r#"{{"agent_id":"{AGENT}","instrument":"{XYZ}","restriction":"stale_mark",
        "reason":"no_sane_mark","active":true,"risk_clock":"{clock}"}}"#
    );
    let record = RiskAppend {
        event_type: "InstrumentRestrictionChanged",
        schema_version: 1,
        causation_id: Some(insane.event_id.clone()),
        payload: parse(payload.as_bytes()).unwrap(),
    };
    assert_eq!(fold.fold(&insane).unwrap(), vec![record]);
    assert_eq!(readable_head(fold.view()), 2);
}

/// DEC-2310 item 4: §5.2 never reads a not-sane mark's bid, so on MC-R20's held 0.1 BTC the same
/// not-sane mark with two bids gives equal records and equal views. E stays at the allocation, 10000,
/// and the mark sets `stale_mark` at once, which B2b-2 writes.
#[test]
fn a_not_sane_marks_step_ignores_its_bid_on_a_held_position() {
    let case = case("MC-R20");
    let mandate = case_mandate(&case);
    let folded: Vec<(Vec<RiskAppend>, RiskView)> = ["1", "99999"]
        .into_iter()
        .map(|bid| {
            let mut fold = open_case(&case, &mandate, &Crypto);
            let clock = "2026-09-21T04:00:30.000000000Z";
            let insane = mark(2, BTC, clock, bid, false, "crypto");
            let appends = fold.fold(&insane).unwrap();
            (appends, fold.view().clone())
        })
        .collect();
    assert_eq!(folded[0], folded[1]);
    let (appends, view) = &folded[0];
    let types: Vec<&str> = appends.iter().map(|append| append.event_type).collect();
    assert_eq!(types, ["InstrumentRestrictionChanged"]);
    let RiskView::Readable { snapshot, head: 2 } = view else {
        panic!("a held position's not-sane mark is readable: {view:?}");
    };
    assert_eq!(snapshot.agent_equity.to_string(), "10000");
}

/// DEC-2313 item 1: `open` binds only a canonical asset ID (lowercase `8-4-4-4-12` hexadecimal), so a
/// held MC-R20 opening on the symbol, or on the asset ID in uppercase, is refused, and on the asset
/// ID as the journal writes it is opened.
#[test]
fn an_opening_on_an_instrument_that_is_not_a_canonical_asset_id_is_refused() {
    let case = case("MC-R20");
    let mandate = case_mandate(&case);
    let refused = Some(SpecError::InvalidInput { what: "instrument" });
    let bad = [
        "BTC/USD".to_owned(),
        BTC.to_uppercase(),
        BTC.replace('-', ""),
        format!("{BTC}0"),
        BTC[..35].to_owned(),
        BTC.replacen('a', "g", 1),
        format!(" {BTC}"),
        format!("{BTC}\n"),
        String::new(),
    ];
    for held in &bad {
        let opened = opened(&case, &mandate, &Crypto, held);
        assert_eq!(opened.err(), refused, "{held:?}");
    }
    assert!(opened(&case, &mandate, &Crypto, BTC).is_ok());
    let flat = |named: &str| {
        AccountRiskFold::open(
            ACCOUNT,
            1,
            AGENT,
            &mandate,
            &opening(),
            Some(named),
            &Wall,
            TripwireState::default(),
        )
        .err()
    };
    assert_eq!(
        flat("XYZ"),
        refused,
        "a flat opening names no symbol either"
    );
    assert_eq!(flat(XYZ), None, "a flat opening on a canonical id");
}

const AT_1: &str = "2026-09-22T14:31:00.000000000Z";
const AT_2: &str = "2026-09-22T14:32:00.000000000Z";
const AT_3: &str = "2026-09-22T14:33:00.000000000Z";

/// The flat opening of `research_equity_pinned` (A 10000), marked at 100 and holding the agent's
/// own order `o-1` for XYZ: seqs 2 to 4.
fn marked_with_an_order<'c>(mandate: &ValidatedMandate, calendar: &'c Wall) -> AccountRiskFold<'c> {
    let mut fold = open(mandate, calendar);
    fold.fold(&mark(2, XYZ, AT_1, "100", true, "regular"))
        .unwrap();
    for row in own_order(3, AGENT, "o-1", XYZ, "buy", "12") {
        assert_eq!(fold.fold(&row).unwrap(), Vec::new());
    }
    fold
}

/// DEC-2393 items 1 to 3, by hand: E = A + cash + Q × mark, where cash is every own fill's
/// proceeds less its cost less every fee charged to an own order.
#[test]
fn the_agents_own_fills_and_fees_move_its_equity_and_another_agents_never_do() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    assert_eq!(equity(fold.view()), "10000");
    fold.fold(&fill(5, "o-1", XYZ, ("buy", "10", "100"), AT_2, "[]"))
        .unwrap();
    assert_eq!(equity(fold.view()), "10000", "10000 - 1000 + 10 × 100");
    fold.fold(&fees_charged(6, "o-1", "0.05", AT_2)).unwrap();
    assert_eq!(equity(fold.view()), "9999.95", "the simulated fee lowers E");
    let commission = r#"[{"kind":"commission","amount":"1.5","asset":"USD","status":"charged"}]"#;
    fold.fold(&fill(7, "o-1", XYZ, ("buy", "2", "100"), AT_2, commission))
        .unwrap();
    assert_eq!(
        equity(fold.view()),
        "9998.45",
        "and so does the fill's own commission"
    );
    for row in own_order(8, "agent-2", "o-2", XYZ, "buy", "5") {
        fold.fold(&row).unwrap();
    }
    fold.fold(&fill(10, "o-2", XYZ, ("buy", "5", "90"), AT_2, commission))
        .unwrap();
    fold.fold(&fees_charged(11, "o-2", "7", AT_2)).unwrap();
    fold.fold(&fill(12, "o-stray", XYZ, ("sell", "1", "1"), AT_2, "[]"))
        .unwrap();
    assert_eq!(
        equity(fold.view()),
        "9998.45",
        "another agent's fills and fees are not this one's"
    );
    fold.fold(&mark(13, XYZ, AT_3, "99", true, "regular"))
        .unwrap();
    assert_eq!(equity(fold.view()), "9986.45", "10000 - 1201.55 + 12 × 99");
    assert_eq!(readable_head(fold.view()), 13);
}

/// An order with no companion is the agent's when the submission itself names it, and a
/// replacement of the agent's order is the agent's (DEC-2393 item 1).
#[test]
fn a_submission_naming_the_agent_and_a_replacement_of_its_order_are_its_own() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    fold.fold(&mark(2, XYZ, AT_1, "100", true, "regular"))
        .unwrap();
    let legacy = row(
        3,
        "OrderSubmitted",
        1,
        &format!(
            r#"{{"client_order_id":"o-1","agent_id":"{AGENT}","instrument_id":"{XYZ}","side":"buy",
            "type":"limit","tif":"day","qty":"10","limit_price":"100","risk_clock":"{OPENED}"}}"#
        ),
    );
    fold.fold(&legacy).unwrap();
    let replaced = row(
        4,
        "OrderStateChanged",
        2,
        r#"{"client_order_id":"o-1b","state":"accepted","replaces":"o-1","ignored":false}"#,
    );
    fold.fold(&replaced).unwrap();
    fold.fold(&fill(5, "o-1", XYZ, ("buy", "4", "100"), AT_2, "[]"))
        .unwrap();
    fold.fold(&fill(6, "o-1b", XYZ, ("buy", "6", "100"), AT_2, "[]"))
        .unwrap();
    fold.fold(&mark(7, XYZ, AT_3, "90", true, "regular"))
        .unwrap();
    assert_eq!(equity(fold.view()), "9900", "10000 - 1000 + 10 × 90");
}

#[test]
fn a_fee_in_another_asset_is_a_refusal_to_decide() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    let in_asset = r#"[{"kind":"crypto_asset","amount":"0.01","asset":"XYZ","status":"charged"}]"#;
    let appended = fold.fold(&fill(5, "o-1", XYZ, ("buy", "1", "100"), AT_2, in_asset));
    assert_eq!(appended.unwrap(), Vec::new());
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 4)
    );
}

#[test]
fn an_own_fill_on_a_second_instrument_is_a_refusal_to_decide() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    fold.fold(&fill(5, "o-1", ETH, ("buy", "1", "100"), AT_2, "[]"))
        .unwrap();
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 4)
    );
}

/// DEC-2314: a flat opening binds only a canonical asset id, whichever row binds it first.
#[test]
fn a_first_binding_on_an_id_that_is_not_canonical_is_a_refusal_to_decide() {
    let mandate = mandate();
    for first in ["XYZ", "7B4A1C2E-2222-4A2B-9C3D-000000000002"] {
        let mut by_mark = open(&mandate, &Wall);
        by_mark
            .fold(&mark(2, first, AT_1, "100", true, "regular"))
            .unwrap();
        assert_eq!(
            unreadable(by_mark.view()),
            (UnreadableCause::RefusedToDecide, 1)
        );

        let mut by_fill = open(&mandate, &Wall);
        for row in own_order(2, AGENT, "o-1", first, "buy", "1") {
            by_fill.fold(&row).unwrap();
        }
        by_fill
            .fold(&fill(4, "o-1", first, ("buy", "1", "100"), AT_2, "[]"))
            .unwrap();
        assert_eq!(
            unreadable(by_fill.view()),
            (UnreadableCause::RefusedToDecide, 3)
        );
    }
}

/// A sale of more than the sub-ledger holds would be a short sale (`AGENTS.md` rule 12).
#[test]
fn an_own_sale_of_more_than_the_agent_holds_is_a_refusal_to_decide() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    fold.fold(&fill(5, "o-1", XYZ, ("sell", "1", "100"), AT_2, "[]"))
        .unwrap();
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 4)
    );
}

#[test]
fn a_fill_or_a_fee_that_does_not_decode_makes_the_view_unreadable() {
    let mandate = mandate();
    let bad_fee = r#"[{"kind":"commission","amount":"-1","asset":"USD","status":"charged"}]"#;
    for row in [
        fill(5, "o-1", XYZ, ("short", "1", "100"), AT_2, "[]"),
        fill(5, "o-1", XYZ, ("buy", "0", "100"), AT_2, "[]"),
        fill(5, "o-1", XYZ, ("buy", "1", "x"), AT_2, "[]"),
        fill(
            5,
            "o-1",
            XYZ,
            ("buy", "1", "100"),
            "2026-09-22T14:32:00.500000000Z",
            "[]",
        ),
        fill(5, "o-1", XYZ, ("buy", "1", "100"), AT_2, bad_fee),
        fill(5, "o-1", XYZ, ("buy", "1", "100"), AT_2, "null"),
        fees_charged(5, "o-1", "-0.01", AT_2),
    ] {
        let mut fold = marked_with_an_order(&mandate, &Wall);
        fold.fold(&row).unwrap();
        assert_eq!(
            unreadable(fold.view()),
            (UnreadableCause::Undecodable, 4),
            "{}",
            String::from_utf8_lossy(&row.body)
        );
    }
}

#[test]
fn an_order_record_at_a_version_the_fold_does_not_know_makes_the_view_unreadable() {
    let mandate = mandate();
    for (event_type, version) in [
        ("OrderRequestRecorded", 2),
        ("OrderSubmitted", 3),
        ("OrderStateChanged", 3),
        ("FillApplied", 2),
        ("FeesCharged", 2),
    ] {
        let mut fold = open(&mandate, &Wall);
        fold.fold(&row(2, event_type, version, "{}")).unwrap();
        assert_eq!(
            unreadable(fold.view()),
            (UnreadableCause::UnknownSchemaVersion, 1),
            "{event_type} v{version}"
        );
    }
}

/// The reference cases that hold a fill, through journaled rows up to their first input that R2
/// reads: each fill is the agent's own order, so the figures are the harness's (DEC-2393 item 4).
/// A case's leading `risk_day_started` at its opening instant is the day the fold opens in, which
/// no tick crosses into, as the MC-R20 walk above skips it.
#[test]
fn the_risk_state_cases_with_fills_fold_through_journaled_rows() {
    let readable = ["mark", "clock", "risk_day_started", "fill"];
    for id in ["MC-R14", "MC-R21", "MC-R23"] {
        let case = case(id);
        let mandate = case_mandate(&case);
        let calendar = nyse();
        let mut fold = open_case(&case, &mandate, &calendar);
        let steps: Vec<Json> = case["steps"]
            .as_array()
            .unwrap()
            .iter()
            .skip_while(|step| {
                step["event"] == "risk_day_started" && step["at"] == case["initial"]["at"]
            })
            .take_while(|step| readable.contains(&step["event"].as_str().unwrap()))
            .cloned()
            .collect();
        assert!(
            steps.iter().any(|step| step["event"] == "fill"),
            "{id} walks a fill"
        );
        walk(&mut fold, instrument(&case), &steps);
    }
}
