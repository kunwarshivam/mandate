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
    AccountRiskFold, ExchangeRiskCalendar, OrderPath, RiskAppend, RiskCalendar, RiskView,
    UnreadableCause, risk_snapshot,
};
use mandate_executor::tripwire::TripwireState;
use mandate_journal::{Draft, StoredEvent};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_spec::context::stop_limit_asset_classes;
use mandate_spec::document::ProvenanceMap;
use mandate_spec::risk::{LimitKey, Opening, RiskState, SessionClock};
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
/// snapshot, `AgentModeApplied` is the executor's own and names as its restriction the action of the
/// step's last trigger before it (DEC-2308 item 3), or with none, the last lift's action or limit,
/// or `goal_completed` (DEC-2331), and an instrument restriction names the
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
    let (mut action, mut cause) = (None, None);
    for (append, entry) in appends.iter().zip(journal) {
        let mode = append.event_type == "AgentModeApplied";
        match entry["type"].as_str() {
            Some("RiskLimitTriggered") => action = entry["action"].as_str(),
            Some("RiskLimitLifted") => cause = entry["action"].as_str().or(entry["limit"].as_str()),
            Some("GoalCompleted") => cause = Some("goal_completed"),
            _ => {}
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
                let wanted = action.or(cause);
                assert_eq!(
                    text("restriction"),
                    wanted,
                    "seq {seq}: DEC-2308 item 3, DEC-2331"
                );
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
        seq = journal(fold, &appends, seq + 1);
        records.push(appends);
    }
    records
}

/// The records a step returned, appended from seq `first` as the executor appends them, in one
/// batch right after their cause, and folded back, each the record the fold owes and stepping
/// nothing (DEC-2301 item 2). Returns the seq after them.
fn journal(fold: &mut AccountRiskFold<'_>, appends: &[RiskAppend], first: u64) -> u64 {
    let before = fold.view().clone();
    let mut seq = first;
    for append in appends {
        let row = appended(seq, append);
        assert_eq!(
            fold.fold(&row).unwrap(),
            Vec::new(),
            "seq {seq}: a record steps nothing"
        );
        seq += 1;
    }
    assert_eq!(fold.owed().count(), 0, "every owed record came back");
    if let (RiskView::Readable { snapshot: was, .. }, RiskView::Readable { snapshot, .. }) =
        (&before, fold.view())
    {
        assert_eq!(was, snapshot, "the records move nothing");
    }
    seq
}

/// `append` as the row the journal stores it as at `seq`.
fn appended(seq: u64, append: &RiskAppend) -> StoredEvent {
    let cause = append.causation_id.clone().map_or(Value::Null, Value::Str);
    let mut body = mandate_canon::Object::new();
    for (key, value) in [
        ("causation_id", cause),
        ("event_type", Value::Str(append.event_type.to_owned())),
        ("payload", append.payload.clone()),
        ("stream_id", Value::Str(ACCOUNT.to_owned())),
    ] {
        body.insert(Key::new(key).unwrap(), value);
    }
    stored(
        seq,
        append.event_type,
        append.schema_version,
        to_canonical(&Value::Object(body)),
    )
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

/// A `FeesCharged` as the paper executor's `simulated_fee` books it for `order`: `accrued` the fee,
/// `charged` 0, `simulated` true (DEC-948 item 3).
fn fees_charged(seq: u64, order: &str, accrued: &str, clock: &str) -> StoredEvent {
    row(
        seq,
        "FeesCharged",
        1,
        &format!(
            r#"{{"family":"equities","day":"{OPENED_DAY}","accrued":"{accrued}","charged":"0",
            "simulated":true,"client_order_id":"{order}","sec":null,"taf":null,"cat":"{accrued}",
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
    let next = journal(&mut fold, &midnight, 5);
    walk_from(&mut fold, XYZ, next, &steps[2..]);
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
        fees_charged(5, "o-1", "x", AT_2),
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

/// Each `RiskLimitTriggered` of a walk as `limit=breach_time_s`, in the order it was appended.
fn breach_times(records: &[Vec<RiskAppend>]) -> Vec<String> {
    let triggers = records.iter().flatten();
    let triggers = triggers.filter(|append| append.event_type == "RiskLimitTriggered");
    triggers
        .map(|append| {
            let limit = append.payload.get("limit").and_then(Value::as_str).unwrap();
            let breach = append.payload.get("breach_time_s").map(to_canonical);
            format!("{limit}={}", String::from_utf8(breach.unwrap()).unwrap())
        })
        .collect()
}

/// MC-R04 (`breach_confirm_s` 60, `at` 0.06 on rung 1): DD is 0.0714 at 14:02:00, so rung 1's breach
/// time runs from there. At 14:02:05 DD reaches 1.25 × 0.06 on a sane quote, `hard_breach_pending`
/// with the 5 s accumulated, and at 14:02:15 the hard level holds 10 s later, `hard_trigger` with
/// 15 s (§5.6, §9.27). The scale rung has no confirmation: 0 s.
#[test]
fn mc_r04s_hard_triggers_carry_the_breach_time_they_accumulated() {
    let case = case("MC-R04");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let mut fold = open_case(&case, &mandate, &calendar);
    let records = walk(&mut fold, XYZ, case["steps"].as_array().unwrap());
    let wanted = [
        "drawdown_ladder[0]=0",
        "drawdown_ladder[1]=5",
        "drawdown_ladder[1]=15",
    ];
    assert_eq!(breach_times(&records), wanted);
}

/// MC-R06 (`breach_confirm_s` 60, crypto): the daily loss breaches at 03:59:30, 30 s before the
/// rollover, and keeps confirming against the previous day's E₀ (§5.4). Its next input is the mark
/// at 04:30:00, so it latches `resolved_at_rollover` with 30 + 1800 = 1830 s.
#[test]
fn mc_r06s_rollover_trigger_carries_the_breach_time_across_midnight() {
    let case = case("MC-R06");
    let mandate = case_mandate(&case);
    let mut fold = open_case(&case, &mandate, &Crypto);
    let steps = case["steps"].as_array().unwrap();
    check_snapshot(fold.view(), 1, &steps[0]["expect"]);
    let records = walk(&mut fold, BTC, &steps[1..]);
    assert_eq!(breach_times(&records), ["max_daily_loss=1830"]);
}

/// MC-R07 and MC-R08 set `breach_confirm_s` to 0, so each limit triggers at the first breaching
/// input with no time in breach before it: the daily loss at 03:50, and MC-R08's new-day breach at
/// 04:20, whose interval from the 04:00 rollover starts with E = E₀. MC-R07's last step, a lift
/// whose `AgentModeApplied` follows no trigger, is DEC-2331 item 1(a)'s.
#[test]
fn zero_confirmation_triggers_carry_no_breach_time() {
    let mut walked = Vec::new();
    for (id, last) in [("MC-R07", 3), ("MC-R08", 4)] {
        let case = case(id);
        let mandate = case_mandate(&case);
        let mut fold = open_case(&case, &mandate, &Crypto);
        let steps = case["steps"].as_array().unwrap();
        walked.push(breach_times(&walk(&mut fold, BTC, &steps[1..=last])));
    }
    assert_eq!(walked[0], ["max_daily_loss=0"]);
    let wanted = [
        "max_daily_loss=0",
        "drawdown_ladder[0]=0",
        "max_daily_loss=0",
    ];
    assert_eq!(walked[1], wanted);
}

/// MC-R22: a `profit_stop` confirms by time in breach on marks alone and journals `GoalCompleted`
/// in its second form, `reason` and `then` with no `on_complete` (§9.27 rule 238), then
/// `AgentModeApplied` to `exits_only`. That mode change follows no trigger, so its restriction is
/// `goal_completed` (DEC-2331 item 1(b)).
#[test]
fn mc_r22_profit_stop_completes_the_goal_on_marks() {
    let case = case("MC-R22");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let mut fold = open_case(&case, &mandate, &calendar);
    let steps = case["steps"].as_array().unwrap();
    walk(&mut fold, XYZ, &steps[..3]);
    let text = |key: &str| steps[3][key].as_str().unwrap();
    let last = mark(5, XYZ, text("at"), text("bid"), true, "regular");
    let appends = fold.fold(&last).unwrap();
    let types: Vec<&str> = appends.iter().map(|append| append.event_type).collect();
    assert_eq!(types, ["GoalCompleted", "AgentModeApplied"]);
    let payload = format!(
        r#"{{"agent_id":"{AGENT}","reason":"profit_stop_reached",
        "then":"discretionary_exit_all_then_retire","on_complete":null,"risk_clock":"{}"}}"#,
        text("at")
    );
    assert_eq!(appends[0].payload, parse(payload.as_bytes()).unwrap());
    let base = base_draft();
    for append in &appends {
        assert_eq!(append.causation_id.as_deref(), Some(last.event_id.as_str()));
        assert!(accepted(&base, append), "{append:?}");
    }
    let mode = |member: &str| appends[1].payload.get(member).cloned();
    assert_eq!(mode("to"), Some(Value::Str("exits_only".to_owned())));
    assert_eq!(mode("originated"), Some(Value::Bool(true)));
    assert_eq!(
        mode("restriction"),
        Some(Value::Str("goal_completed".to_owned()))
    );
    check_snapshot(fold.view(), 5, &steps[3]["expect"]);
}

/// DEC-2331 item 1(a): a mode change with no trigger before it in its step names the step's last
/// lift: its ladder action, or its limit's journal name when the lift carries none. MC-R07's last
/// mark lifts `max_daily_loss`, which carries no action, back to `normal`; MC-R19's last mark clears
/// the flash print's hard breach, and its last lift, `drawdown_ladder[2]` with `hard_breach_cleared`,
/// carries none either.
#[test]
fn a_mode_change_after_a_lift_names_the_lifted_limit() {
    let mut named = Vec::new();
    for (id, crypto) in [("MC-R07", true), ("MC-R19", false)] {
        let case = case(id);
        let (mandate, calendar) = (case_mandate(&case), nyse());
        let clock: &dyn RiskCalendar = if crypto { &Crypto } else { &calendar };
        let mut fold = open_case(&case, &mandate, clock);
        let steps = case["steps"].as_array().unwrap();
        let steps = if crypto { &steps[1..] } else { &steps[..] };
        let records = walk(&mut fold, instrument(&case), steps);
        let last = records.last().unwrap().last().unwrap();
        assert_eq!(last.event_type, "AgentModeApplied", "{id}");
        let restriction = last.payload.get("restriction").and_then(Value::as_str);
        named.push(restriction.map(str::to_owned));
    }
    let wanted = ["max_daily_loss", "drawdown_ladder[2]"].map(|name| Some(name.to_owned()));
    assert_eq!(named, wanted);
}

/// The paper executor's own fee record, as `simulated_fee` writes it: `charged` "0" and `accrued`
/// the fee. The accrual lowers E (trading spec §8.2); a broker-posted or another family's fee for
/// an own order is a refusal to decide (DEC-2393 item 3).
#[test]
fn the_simulated_fee_lowers_equity_by_its_accrual_and_any_other_fee_record_is_refused() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    fold.fold(&fill(5, "o-1", XYZ, ("buy", "10", "100"), AT_2, "[]"))
        .unwrap();
    let simulated = row(
        6,
        "FeesCharged",
        1,
        &format!(
            r#"{{"family":"equities","day":"{OPENED_DAY}","accrued":"0.0123","charged":"0",
            "simulated":true,"client_order_id":"o-1","sec":null,"taf":null,"cat":"0.0123",
            "risk_clock":"{AT_2}"}}"#
        ),
    );
    fold.fold(&simulated).unwrap();
    assert_eq!(equity(fold.view()), "9999.9877");
    for (simulated, family) in [("false", "equities"), ("true", "crypto")] {
        let mut fold = marked_with_an_order(&mandate, &Wall);
        let posted = row(
            5,
            "FeesCharged",
            1,
            &format!(
                r#"{{"family":"{family}","day":"{OPENED_DAY}","accrued":"1","charged":"1",
                "simulated":{simulated},"client_order_id":"o-1","sec":null,"taf":null,"cat":"1",
                "risk_clock":"{AT_2}"}}"#
            ),
        );
        fold.fold(&posted).unwrap();
        assert_eq!(
            unreadable(fold.view()),
            (UnreadableCause::RefusedToDecide, 4)
        );
    }
}

/// A fill naming no order is external activity (journal spec §9.16), never the agent's; a fill
/// journaled twice is stepped once.
#[test]
fn a_fill_with_no_order_steps_nothing_and_a_repeated_fill_steps_once() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    let external = row(
        5,
        "FillApplied",
        1,
        &format!(
            r#"{{"fill_id":"f-x","client_order_id":null,"instrument_id":"{XYZ}","side":"buy",
            "qty_gross":"5","price":"1","trade_date":"{OPENED_DAY}","risk_clock":"{AT_2}","fees":[]}}"#
        ),
    );
    fold.fold(&external).unwrap();
    assert_eq!(equity(fold.view()), "10000");
    let first = fill(6, "o-1", XYZ, ("buy", "10", "100"), AT_2, "[]");
    fold.fold(&first).unwrap();
    let mut again = fill(7, "o-1", XYZ, ("buy", "10", "100"), AT_2, "[]");
    again.body = String::from_utf8(again.body.clone())
        .unwrap()
        .replace("f-7", "f-6")
        .into_bytes();
    fold.fold(&again).unwrap();
    fold.fold(&mark(8, XYZ, AT_3, "90", true, "regular"))
        .unwrap();
    assert_eq!(
        equity(fold.view()),
        "9900",
        "10 shares, not 20: 10000 - 1000 + 10 × 90"
    );
}

/// A successor named by a change that is not terminal, or is `ignored`, is not the agent's, as the
/// executor's fold names a successor only on a terminal, applied state (DEC-2393 item 1).
#[test]
fn only_a_terminal_applied_change_makes_its_successor_the_agents() {
    let mandate = mandate();
    for (state, ignored) in [("accepted", false), ("replaced", true)] {
        let mut fold = marked_with_an_order(&mandate, &Wall);
        let changed = row(
            5,
            "OrderStateChanged",
            2,
            &format!(
                r#"{{"client_order_id":"o-1","state":"{state}","replaced_by":"o-9","ignored":{ignored}}}"#
            ),
        );
        fold.fold(&changed).unwrap();
        fold.fold(&fill(6, "o-9", XYZ, ("buy", "10", "100"), AT_2, "[]"))
            .unwrap();
        fold.fold(&mark(7, XYZ, AT_3, "90", true, "regular"))
            .unwrap();
        assert_eq!(equity(fold.view()), "10000", "{state}, ignored {ignored}");
    }
}

/// MC-R01 to its `paused`, through rows, with every record it returned journaled back: the fold
/// it leaves, and its view.
fn mc_r01_journaled<'c>(
    mandate: &ValidatedMandate,
    calendar: &'c Nyse,
) -> (AccountRiskFold<'c>, Vec<StoredEvent>) {
    let case = case("MC-R01");
    let mut fold = open_case(&case, mandate, calendar);
    let mut rows = Vec::new();
    let mut seq = 2;
    for step in case["steps"].as_array().unwrap() {
        let text = |key: &str| step[key].as_str().unwrap();
        let row = mark(seq, XYZ, text("at"), text("bid"), true, "regular");
        let appends = fold.fold(&row).unwrap();
        rows.push(row);
        seq += 1;
        for append in &appends {
            let record = appended(seq, append);
            fold.fold(&record).unwrap();
            rows.push(record);
            seq += 1;
        }
    }
    (fold, rows)
}

/// A replay of the journal from seq 1 re-derives every record the live fold appended, from its
/// cause, finds each on the stream as it owed it, and ends at the same view owing nothing, so the
/// executor appends nothing twice (DEC-2301 item 2, DEC-2304; R3).
#[test]
fn a_replay_of_the_journal_reproduces_the_live_view() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (live, rows) = mc_r01_journaled(&mandate, &calendar);
    assert!(
        rows.iter()
            .any(|row| row.event_type == "RiskLimitTriggered"),
        "the walk journals records"
    );
    let mut replayed = open_case(&case, &mandate, &calendar);
    let mut returned = 0;
    for row in &rows {
        returned += replayed.fold(row).unwrap().len();
    }
    let journaled = rows
        .iter()
        .filter(|row| row.event_type.starts_with("Risk") || row.event_type == "AgentModeApplied")
        .count();
    assert_eq!(
        returned, journaled,
        "each journaled record re-derived once, from its cause"
    );
    assert_eq!(replayed.view(), live.view());
    assert_eq!(replayed.owed().count(), 0);
}

/// Every way a journaled record can differ from the one the fold owes makes the view unreadable
/// (`not_reproduced`): its payload, its cause, its type, its version, its order, and a record the
/// fold never returned.
#[test]
fn a_record_the_fold_would_not_have_produced_makes_the_view_unreadable() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (_, rows) = mc_r01_journaled(&mandate, &calendar);
    let first = rows
        .iter()
        .position(|row| row.event_type == "RiskLimitTriggered")
        .unwrap();
    let tampered = |change: &dyn Fn(&mut StoredEvent)| {
        let mut rows = rows.clone();
        change(&mut rows[first]);
        let mut fold = open_case(&case, &mandate, &calendar);
        for row in &rows[..=first] {
            fold.fold(row).unwrap();
        }
        unreadable(fold.view())
    };
    let edit = |from: &'static str, to: &'static str| {
        move |row: &mut StoredEvent| {
            let body = String::from_utf8(row.body.clone()).unwrap();
            assert!(body.contains(from), "{from} in {body}");
            row.body = body.replacen(from, to, 1).into_bytes();
        }
    };
    let head = u64::try_from(first).unwrap() + 1;
    let wanted = (UnreadableCause::NotReproduced, head);
    assert_eq!(
        tampered(&edit(r#""agent_equity":""#, r#""agent_equity":"1"#)),
        wanted
    );
    assert_eq!(
        tampered(&edit(r#""causation_id":""#, r#""causation_id":"0"#)),
        wanted
    );
    assert_eq!(tampered(&|row| row.schema_version = 2), wanted);
    assert_eq!(
        tampered(&|row| row.event_type = "RiskLimitLifted".to_owned()),
        wanted
    );
    let swapped = {
        let mut rows = rows.clone();
        let (first, len) = burst(&rows);
        assert!(len >= 2, "two records in a row");
        let (a, b) = (rows[first].clone(), rows[first + 1].clone());
        rows[first] = StoredEvent { seq: a.seq, ..b };
        rows[first + 1] = StoredEvent {
            seq: a.seq + 1,
            ..a
        };
        let mut fold = open_case(&case, &mandate, &calendar);
        for row in &rows[..=first] {
            fold.fold(row).unwrap();
        }
        (unreadable(fold.view()), first)
    };
    let head = u64::try_from(swapped.1).unwrap() + 1;
    assert_eq!(
        swapped.0,
        (UnreadableCause::NotReproduced, head),
        "records out of the order owed"
    );

    let mut forged = open_case(&case, &mandate, &calendar);
    let mut record = rows[first].clone();
    record.seq = 2;
    forged.fold(&record).unwrap();
    assert_eq!(
        unreadable(forged.view()),
        (UnreadableCause::NotReproduced, 1),
        "a record the fold never returned"
    );
}

/// A crash between a cause and its records leaves them owed at the head: the restarted fold reads
/// the stream to its end and names exactly the records to append next (R3).
#[test]
fn records_a_crash_kept_from_the_journal_are_owed_at_the_head() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (live, rows) = mc_r01_journaled(&mandate, &calendar);
    let first = rows
        .iter()
        .position(|row| row.event_type == "RiskLimitTriggered")
        .unwrap();
    let cause = &rows[first - 1];
    let mut restarted = open_case(&case, &mandate, &calendar);
    for row in &rows[..first] {
        restarted.fold(row).unwrap();
    }
    let owed: Vec<RiskAppend> = restarted.owed().cloned().collect();
    assert!(!owed.is_empty());
    assert!(
        owed.iter()
            .all(|append| append.causation_id.as_deref() == Some(cause.event_id.as_str()))
    );
    let mut seq = cause.seq + 1;
    for append in &owed {
        let row = appended(seq, append);
        assert_eq!(
            row.body,
            rows[usize::try_from(seq - 2).unwrap()].body,
            "seq {seq}"
        );
        restarted.fold(&row).unwrap();
        seq += 1;
    }
    for row in &rows[usize::try_from(seq - 2).unwrap()..] {
        restarted.fold(row).unwrap();
    }
    assert_eq!(restarted.view(), live.view());
}

/// An `AgentModeApplied` the fold does not owe is another writer's, an owner's pause or a kill
/// switch, and moves nothing in the fold.
#[test]
fn a_mode_record_the_fold_does_not_owe_is_another_writers() {
    let mandate = mandate();
    let mut fold = open(&mandate, &Wall);
    let before = fold.view().clone();
    let paused = caused(
        2,
        "AgentModeApplied",
        1,
        None,
        &format!(r#"{{"agent":"{AGENT}","to":"paused","restriction":"owner","originated":false}}"#),
    );
    fold.fold(&paused).unwrap();
    let RiskView::Readable { snapshot, head } = fold.view() else {
        panic!("readable");
    };
    let RiskView::Readable { snapshot: was, .. } = &before else {
        panic!("readable");
    };
    assert_eq!((snapshot, *head), (was, 2));
}

/// One generated input: a sane or insane regular mark at a bid, a tick, or an own buy or sell.
#[derive(Debug, Clone)]
enum Generated {
    Mark(u16, bool),
    Tick,
    Buy(u8),
    Sell(u8),
}

fn generated() -> impl proptest::strategy::Strategy<Value = Generated> {
    use proptest::prelude::*;
    prop_oneof![
        (80_u16..=120, any::<bool>()).prop_map(|(bid, sane)| Generated::Mark(bid, sane)),
        Just(Generated::Tick),
        (1_u8..=5).prop_map(Generated::Buy),
        (1_u8..=5).prop_map(Generated::Sell),
    ]
}

/// The rows of `inputs` folded live on MC-R01's mandate, one minute apart from 14:01, each input
/// journaled as the executor journals it, one batch (a fill with its `FeesCharged` and its order's
/// state, a tick with a `TradingDayStarted`), and the records the fold returned for the batch
/// journaled right after it; a sale beyond what the generated buys hold is left out, so every
/// stream is one the executor could write.
fn live_rows(
    inputs: &[Generated],
    mandate: &ValidatedMandate,
    calendar: &Nyse,
) -> (Vec<StoredEvent>, RiskView) {
    let case = case("MC-R01");
    let mut fold = open_case(&case, mandate, calendar);
    let (mut rows, mut seq, mut held) = (Vec::new(), 2_u64, 100_u32);
    for (minute, input) in (1_u32..).zip(inputs) {
        let clock = format!(
            "2026-09-21T{:02}:{:02}:00.000000000Z",
            14 + minute / 60,
            minute % 60
        );
        let mut cause = Vec::new();
        match input {
            Generated::Mark(bid, sane) => {
                cause.push(mark(seq, XYZ, &clock, &bid.to_string(), *sane, "regular"));
            }
            Generated::Tick => cause.push(tick(seq, &clock)),
            Generated::Buy(qty) | Generated::Sell(qty) => {
                let side = if matches!(input, Generated::Buy(_)) {
                    "buy"
                } else {
                    "sell"
                };
                if side == "sell" && u32::from(*qty) > held {
                    continue;
                }
                held = if side == "buy" {
                    held + u32::from(*qty)
                } else {
                    held - u32::from(*qty)
                };
                let order = format!("o-{seq}");
                cause.extend(own_order(seq, AGENT, &order, XYZ, side, &qty.to_string()));
                cause.push(fill(
                    seq + 2,
                    &order,
                    XYZ,
                    (side, &qty.to_string(), "100"),
                    &clock,
                    "[]",
                ));
                cause.push(fees_charged(seq + 3, &order, "0.01", &clock));
                cause.push(row(
                    seq + 4,
                    "OrderStateChanged",
                    2,
                    &format!(r#"{{"client_order_id":"{order}","state":"filled","ignored":false}}"#),
                ));
            }
        }
        if matches!(input, Generated::Tick) {
            cause.push(trading_day(seq + 1, &clock));
        }
        let mut returned = Vec::new();
        for row in cause {
            returned.extend(fold.fold(&row).unwrap());
            seq = row.seq + 1;
            rows.push(row);
        }
        for append in &returned {
            let record = appended(seq, append);
            fold.fold(&record).unwrap();
            rows.push(record);
            seq += 1;
        }
    }
    (rows, fold.view().clone())
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

    /// Any journal the executor writes replays from seq 1 to the live view, owing nothing; and cut
    /// at any row, the restarted fold owes exactly the journaled records the cut dropped whose
    /// cause it kept, in their order (DEC-2399). A crash cuts only between batches, since an append
    /// is atomic; cutting inside a batch too is the stricter check.
    #[test]
    fn a_replay_ends_where_the_live_fold_did_and_a_crash_owes_what_it_dropped(
        inputs in proptest::collection::vec(generated(), 1..40),
        cut in 0_usize..1000,
    ) {
        let case = case("MC-R01");
        let (mandate, calendar) = (case_mandate(&case), nyse());
        let (rows, live) = live_rows(&inputs, &mandate, &calendar);
        proptest::prop_assert!(
            matches!(live, RiskView::Readable { .. }),
            "a stream the executor writes stays readable: {:?}",
            live
        );
        let mut replayed = open_case(&case, &mandate, &calendar);
        for row in &rows {
            replayed.fold(row).unwrap();
        }
        proptest::prop_assert_eq!(replayed.view(), &live);
        proptest::prop_assert_eq!(replayed.owed().count(), 0);

        let kept = rows.len().saturating_sub(1) * cut / 1000;
        let mut restarted = open_case(&case, &mandate, &calendar);
        for row in &rows[..kept] {
            restarted.fold(row).unwrap();
        }
        let kept_ids: BTreeSet<&str> = rows[..kept].iter().map(|row| row.event_id.as_str()).collect();
        let dropped: Vec<&StoredEvent> = rows[kept..]
            .iter()
            .filter(|row| FOLD_TYPES.contains(&row.event_type.as_str()))
            .filter(|row| {
                let body = parse(&row.body).unwrap();
                body.get("causation_id")
                    .and_then(Value::as_str)
                    .is_some_and(|cause| kept_ids.contains(cause))
            })
            .collect();
        let owed: Vec<RiskAppend> = restarted.owed().cloned().collect();
        proptest::prop_assert_eq!(owed.len(), dropped.len());
        for (append, row) in owed.iter().zip(&dropped) {
            proptest::prop_assert_eq!(&appended(row.seq, append).body, &row.body);
        }
    }
}

/// The record types the fold returns, written here apart from the crate's list.
const FOLD_TYPES: [&str; 8] = [
    "RiskDayStarted",
    "RiskLimitTriggered",
    "RiskLimitLifted",
    "HighWaterMarkReset",
    "PositionReleased",
    "InstrumentRestrictionChanged",
    "GoalCompleted",
    "AgentModeApplied",
];

/// The scheduler's `TradingDayStarted` copy, which the executor journals in the batch right after
/// the tick that crosses midnight (journal spec §9.13).
fn trading_day(seq: u64, clock: &str) -> StoredEvent {
    row(
        seq,
        "TradingDayStarted",
        1,
        &format!(r#"{{"day":"2026-09-23","clock":"{clock}"}}"#),
    )
}

/// The executor journals a whole batch before the fold reads it, so the rest of the cause's batch
/// comes between the cause and its records: a fill's `FeesCharged` and its order's state, and a
/// midnight tick's `TradingDayStarted`. They pass, and the records are found after them (DEC-2399
/// item 2).
#[test]
fn the_rest_of_a_causes_batch_comes_before_its_records() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    let filled = row(
        7,
        "OrderStateChanged",
        2,
        r#"{"client_order_id":"o-1","state":"filled","ignored":false}"#,
    );
    let batch = [
        fill(5, "o-1", XYZ, ("buy", "12", "100"), AT_2, "[]"),
        fees_charged(6, "o-1", "0.01", AT_2),
        filled,
        tick(8, "2026-09-23T04:00:00.000000000Z"),
        trading_day(9, "2026-09-23T04:00:00.000000000Z"),
    ];
    let checked = row(10, "ConnectionChecked", 1, r#"{"result":"ok"}"#);
    let mut returned = Vec::new();
    for row in &batch {
        returned.extend(fold.fold(row).unwrap());
    }
    assert_eq!(
        returned.first().map(|append| append.event_type),
        Some("RiskDayStarted"),
        "the midnight tick's records, owed after the batch"
    );
    assert_eq!(fold.owed().count(), returned.len());
    fold.fold(&checked).unwrap();
    assert_eq!(
        fold.owed().count(),
        returned.len(),
        "a connection check passes too"
    );
    let next = journal(&mut fold, &returned, 11);
    assert_eq!(readable_head(fold.view()), next - 1);
}

/// An `AgentModeApplied` is the fold's only when it names the cause of the record owed next, and
/// then it must be that record. One naming no cause (a kill switch, an unmapped broker status) or
/// another cause (the copy of an agent stream's `AgentModeChanged`, journal spec §9.6) is another
/// writer's, whether or not records are owed (DEC-2399 items 1 and 4).
#[test]
fn a_mode_record_not_naming_the_owed_cause_is_another_writers_even_while_records_are_owed() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (_, rows) = mc_r01_journaled(&mandate, &calendar);
    let first = rows
        .iter()
        .position(|row| row.event_type == "RiskLimitTriggered")
        .unwrap();
    let others = |seq: u64, cause: Option<&str>| {
        caused(
            seq,
            "AgentModeApplied",
            1,
            cause,
            &format!(
                r#"{{"agent":"{AGENT}","to":"paused","restriction":"kill_switch","originated":true}}"#
            ),
        )
    };
    let replay = |extra: StoredEvent| {
        let mut fold = open_case(&case, &mandate, &calendar);
        for row in &rows[..first] {
            fold.fold(row).unwrap();
        }
        fold.fold(&extra).unwrap();
        for row in &rows[first..] {
            let moved = StoredEvent {
                seq: row.seq + 1,
                ..row.clone()
            };
            fold.fold(&moved).unwrap();
        }
        fold
    };
    let copied = caused(
        rows[first].seq,
        "AgentModeApplied",
        1,
        Some("01J8Z4C0000000000000000000"),
        &format!(r#"{{"agent":"{AGENT}","to":"paused","restriction":"owner","originated":false}}"#),
    );
    let passed = replay(copied);
    assert!(
        matches!(passed.view(), RiskView::Readable { .. }),
        "a copy naming another cause: {:?}",
        passed.view()
    );
    let passed = replay(others(rows[first].seq, None));
    assert!(
        matches!(passed.view(), RiskView::Readable { .. }),
        "{:?}",
        passed.view()
    );
    let cause = rows[first - 1].event_id.clone();
    let refused = replay(others(rows[first].seq, Some(&cause)));
    assert_eq!(
        unreadable(refused.view()).0,
        UnreadableCause::NotReproduced,
        "a caused mode record that is not the one owed"
    );
}

/// An unreadable view owes nothing, after a failed append or after any other cause, so the
/// executor never appends what is no longer the journal's (DEC-2399 item 3).
#[test]
fn an_unreadable_view_owes_nothing() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (_, rows) = mc_r01_journaled(&mandate, &calendar);
    let first = rows
        .iter()
        .position(|row| row.event_type == "RiskLimitTriggered")
        .unwrap();
    let owing = || {
        let mut fold = open_case(&case, &mandate, &calendar);
        for row in &rows[..first] {
            fold.fold(row).unwrap();
        }
        assert!(fold.owed().count() > 0);
        fold
    };
    let mut failed = owing();
    failed.append_failed();
    assert_eq!(failed.owed().count(), 0);
    let mut gap = owing();
    let skipped = StoredEvent {
        seq: rows[first].seq + 1,
        ..rows[first].clone()
    };
    gap.fold(&skipped).unwrap();
    assert_eq!(unreadable(gap.view()).0, UnreadableCause::SeqGap);
    assert_eq!(gap.owed().count(), 0);
}

/// The executor's restart, end to end: the journal is cut after a cause's batch, part or none of
/// its records appended; the restarted fold reads to the head, the owed records are appended
/// there, and a second replay of the whole repaired journal, from seq 1, reaches the live view
/// owing nothing. The oracle is the live run's own journal, whose records came from what the
/// fold returned, not from `owed()`.
#[test]
fn a_journal_repaired_at_the_head_replays_to_the_live_view() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (live, rows) = mc_r01_journaled(&mandate, &calendar);
    let (first, records) = burst(&rows);
    assert!(records >= 2, "the cut can fall inside the batch of records");
    for appended_before_crash in 0..records {
        let cut = first + appended_before_crash;
        let mut restarted = open_case(&case, &mandate, &calendar);
        for row in &rows[..cut] {
            restarted.fold(row).unwrap();
        }
        let owed: Vec<RiskAppend> = restarted.owed().cloned().collect();
        assert_eq!(owed.len(), records - appended_before_crash);
        let mut repaired: Vec<StoredEvent> = rows[..cut].to_vec();
        for (seq, append) in (rows[cut].seq..).zip(&owed) {
            let row = appended(seq, append);
            assert_eq!(row.body, rows[usize::try_from(seq - 2).unwrap()].body);
            restarted.fold(&row).unwrap();
            repaired.push(row);
        }
        repaired.extend(rows[first + records..].iter().cloned());
        let mut again = open_case(&case, &mandate, &calendar);
        for row in &repaired {
            again.fold(row).unwrap();
        }
        assert_eq!(
            again.view(),
            live.view(),
            "cut after {appended_before_crash} records"
        );
        assert_eq!(again.owed().count(), 0);
    }
}

/// The longest run of consecutive risk records in `rows`: where it starts, and how long it is.
fn burst(rows: &[StoredEvent]) -> (usize, usize) {
    let mut best = (0, 0);
    let mut start = 0;
    for (index, row) in rows.iter().enumerate() {
        if !FOLD_TYPES.contains(&row.event_type.as_str()) {
            start = index + 1;
        } else if index + 1 - start > best.1 {
            best = (start, index + 1 - start);
        }
    }
    best
}

/// Another writer's mode record moves nothing in the fold: one that would loosen it, to `normal`
/// while the ladder holds the agent `paused`, neither lifts a latched rung nor changes the fold's
/// mode, which only the risk state's own steps move (DEC-2399 item 4).
#[test]
fn another_writers_mode_record_never_loosens_the_fold() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (mut fold, rows) = mc_r01_journaled(&mandate, &calendar);
    let RiskView::Readable {
        snapshot: before, ..
    } = fold.view().clone()
    else {
        panic!("readable");
    };
    assert_eq!(before.agent_mode, AgentMode::Paused);
    assert!(!before.latched.is_empty());
    let seq = rows.last().unwrap().seq + 1;
    for (offset, cause) in [(0_u64, None), (1, Some("01J8Z4C0000000000000000000"))] {
        let loosen = caused(
            seq + offset,
            "AgentModeApplied",
            1,
            cause,
            &format!(
                r#"{{"agent":"{AGENT}","to":"normal","restriction":"owner","originated":false}}"#
            ),
        );
        fold.fold(&loosen).unwrap();
    }
    let RiskView::Readable { snapshot, head } = fold.view() else {
        panic!("readable: {:?}", fold.view());
    };
    assert_eq!((snapshot.as_ref(), *head), (before.as_ref(), seq + 1));
    assert!(fold.view().holds(OrderPath::Opening));
}

/// A mode record naming the cause of any record owed is the fold's own, so it must be the record
/// owed next: one naming a later owed record's cause, arriving first, is `not_reproduced`.
#[test]
fn a_mode_record_naming_a_later_owed_cause_is_not_reproduced() {
    let mandate = mandate();
    let mut fold = marked_with_an_order(&mandate, &Wall);
    let batch = [
        tick(5, "2026-09-23T04:00:00.000000000Z"),
        fill(
            6,
            "o-1",
            XYZ,
            ("buy", "12", "100"),
            "2026-09-23T04:00:00.000000000Z",
            "[]",
        ),
        mark(
            7,
            XYZ,
            "2026-09-23T04:00:01.000000000Z",
            "50",
            true,
            "regular",
        ),
    ];
    let mut returned = Vec::new();
    for row in &batch {
        returned.extend(fold.fold(row).unwrap());
    }
    let later = returned
        .iter()
        .rfind(|append| append.causation_id.as_deref() == Some(batch[2].event_id.as_str()))
        .expect("the crash's records");
    assert_ne!(returned[0].causation_id, later.causation_id);
    let early = caused(
        8,
        "AgentModeApplied",
        1,
        later.causation_id.as_deref(),
        &format!(r#"{{"agent":"{AGENT}","to":"paused","restriction":"x","originated":true}}"#),
    );
    fold.fold(&early).unwrap();
    assert_eq!(unreadable(fold.view()), (UnreadableCause::NotReproduced, 7));
}

/// A tick or a mark from before the agent's deployment is not its input and steps nothing, a mark
/// of an earlier agent's instrument included; an own fill or fee there cannot be, and is a refusal
/// to decide (DEC-2804 item 3).
#[test]
fn rows_from_before_the_deployment_are_not_the_agents() {
    let mandate = mandate();
    let before = "2026-09-22T14:29:00.000000000Z";
    let mut fold = open(&mandate, &Wall);
    let opened = fold.view().clone();
    fold.fold(&tick(2, before)).unwrap();
    fold.fold(&mark(3, ETH, before, "50", true, "regular"))
        .unwrap();
    let RiskView::Readable { snapshot, head } = fold.view() else {
        panic!("readable");
    };
    let RiskView::Readable { snapshot: was, .. } = &opened else {
        panic!("readable");
    };
    assert_eq!(
        (snapshot, *head),
        (was, 3),
        "E, staleness and the day unmoved"
    );
    for row in own_order(4, AGENT, "o-1", XYZ, "buy", "1") {
        fold.fold(&row).unwrap();
    }
    fold.fold(&fill(6, "o-1", XYZ, ("buy", "1", "100"), before, "[]"))
        .unwrap();
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 5)
    );

    let mut fee = open(&mandate, &Wall);
    for row in own_order(2, AGENT, "o-1", XYZ, "buy", "1") {
        fee.fold(&row).unwrap();
    }
    fee.fold(&fees_charged(4, "o-1", "0.01", before)).unwrap();
    assert_eq!(
        unreadable(fee.view()),
        (UnreadableCause::RefusedToDecide, 3)
    );
}

/// The opening's own second is the agent's: a mark of another instrument there fails closed, and
/// on a bound fold a mark before it, of the bound instrument or another, steps nothing before the
/// instrument is checked (DEC-2804 item 3, DEC-2308 item 4).
#[test]
fn the_opening_second_is_the_agents_and_a_bound_fold_skips_earlier_marks() {
    let mandate = mandate();
    let before = "2026-09-22T14:29:59.000000000Z";
    let bound = || {
        AccountRiskFold::open(
            ACCOUNT,
            1,
            AGENT,
            &mandate,
            &opening(),
            Some(XYZ),
            &Wall,
            TripwireState::default(),
        )
        .unwrap()
    };
    let mut fold = bound();
    let opened = fold.view().clone();
    fold.fold(&mark(2, ETH, before, "50", true, "regular"))
        .unwrap();
    fold.fold(&mark(3, XYZ, before, "99", true, "regular"))
        .unwrap();
    let (RiskView::Readable { snapshot, head }, RiskView::Readable { snapshot: was, .. }) =
        (fold.view(), &opened)
    else {
        panic!("readable");
    };
    assert_eq!(
        (snapshot, *head),
        (was, 3),
        "neither earlier mark is the agent's"
    );
    fold.fold(&mark(4, ETH, OPENED, "50", true, "regular"))
        .unwrap();
    assert_eq!(
        unreadable(fold.view()),
        (UnreadableCause::RefusedToDecide, 3),
        "a mark at the opening second is the agent's, and names another instrument"
    );

    let mut at_opening = bound();
    for row in own_order(2, AGENT, "o-1", XYZ, "buy", "1") {
        at_opening.fold(&row).unwrap();
    }
    at_opening
        .fold(&fill(4, "o-1", XYZ, ("buy", "1", "100"), OPENED, "[]"))
        .unwrap();
    assert!(
        matches!(at_opening.view(), RiskView::Readable { head: 4, .. }),
        "a fill at the opening second is the agent's: {:?}",
        at_opening.view()
    );
    let before = equity(at_opening.view());
    at_opening
        .fold(&fees_charged(5, "o-1", "0.01", OPENED))
        .unwrap();
    let lowered = Usd::parse(&before)
        .unwrap()
        .checked_sub(Usd::parse("0.01").unwrap())
        .unwrap();
    assert_eq!(
        equity(at_opening.view()),
        lowered.to_string(),
        "a fee at the opening second is the agent's, and lowers E by its 0.01"
    );
}

/// Every latched limit and every figure maps to the gate's own, each distinct so a swap between
/// two of them shows (DEC-2804 item 7).
#[test]
fn the_gates_snapshot_maps_every_limit_and_figure() {
    let RiskView::Readable { mut snapshot, .. } = readable_in(AgentMode::ExitsOnly) else {
        panic!("readable");
    };
    let usd = |text: &str| Usd::parse(text).unwrap();
    snapshot.agent_equity = usd("901");
    snapshot.high_water_mark = usd("1002");
    snapshot.day_start_equity = usd("953");
    snapshot.capital_base = usd("1004");
    snapshot.inherited_loss = usd("5");
    snapshot.size_factor = Ratio::parse("0.25").unwrap();
    snapshot.active_rungs = BTreeMap::from([(1, 30), (2, 7)]);
    snapshot.latched = BTreeSet::from([
        LimitKey::MaxDailyLoss,
        LimitKey::DrawdownRung(2),
        LimitKey::LifetimeFloor,
        LimitKey::ProfitStop,
    ]);
    let view = RiskView::Readable { snapshot, head: 9 };
    let (risk, mode) = risk_snapshot(&view).unwrap();
    assert_eq!(mode, mandate_risk::AgentMode::ExitsOnly);
    assert_eq!(
        risk,
        mandate_risk::RiskSnapshot {
            agent_equity: usd("901"),
            high_water_mark: usd("1002"),
            day_start_equity: usd("953"),
            capital_base: usd("1004"),
            inherited_loss: usd("5"),
            latched: BTreeSet::from([
                mandate_risk::spec_types::LimitKey::MaxDailyLoss,
                mandate_risk::spec_types::LimitKey::DrawdownRung(2),
                mandate_risk::spec_types::LimitKey::LifetimeFloor,
                mandate_risk::spec_types::LimitKey::ProfitStop,
            ]),
            active_rungs: BTreeMap::from([(1, 30), (2, 7)]),
            size_factor: Ratio::parse("0.25").unwrap(),
            agent_mode: mandate_risk::AgentMode::ExitsOnly,
        }
    );
}

/// The deployed calendar's edges: the regular session's open and close seconds, a half day, a
/// holiday, a daylight-saving change, a span over several days, and a clock that goes back, which
/// is refused for an equity as for crypto.
#[test]
fn the_deployed_calendar_counts_half_days_holidays_and_edges() {
    let equity = ExchangeRiskCalendar::new(AssetClass::UsEquity).unwrap();
    let between = |from: &str, to: &str| equity.seconds_between(at(from), at(to)).unwrap();
    for (instant, session) in [
        ("2026-09-22T13:29:59.000000000Z", MarketSession::PreMarket),
        ("2026-09-22T13:30:00.000000000Z", MarketSession::Regular),
        ("2026-09-22T19:59:59.000000000Z", MarketSession::Regular),
        ("2026-09-22T20:00:00.000000000Z", MarketSession::AfterHours),
        ("2026-11-26T15:00:00.000000000Z", MarketSession::Overnight),
        ("2026-11-27T18:30:00.000000000Z", MarketSession::AfterHours),
    ] {
        assert_eq!(equity.session(at(instant)).unwrap(), session, "{instant}");
    }
    assert_eq!(
        between(
            "2026-09-22T13:29:00.000000000Z",
            "2026-09-22T13:31:00.000000000Z"
        ),
        60,
        "only the regular session's first minute"
    );
    assert_eq!(
        between(
            "2026-11-26T00:00:00.000000000Z",
            "2026-11-27T00:00:00.000000000Z"
        ),
        0,
        "Thanksgiving"
    );
    assert_eq!(
        between(
            "2026-11-27T00:00:00.000000000Z",
            "2026-11-28T00:00:00.000000000Z"
        ),
        12_600,
        "the half day closes at 13:00 New York"
    );
    assert_eq!(
        between(
            "2026-10-30T14:30:00.000000000Z",
            "2026-11-02T14:31:00.000000000Z"
        ),
        19_800 + 60,
        "Friday from 10:30 New York, the weekend, Monday's first minute after the change to EST"
    );
    assert_eq!(
        between(
            "2026-11-02T14:30:00.000000000Z",
            "2026-11-02T14:31:00.000000000Z"
        ),
        60,
        "09:30 New York is 14:30 UTC after the change"
    );
    for calendar in [
        equity,
        ExchangeRiskCalendar::new(AssetClass::Crypto).unwrap(),
    ] {
        assert_eq!(
            calendar.seconds_between(at(OPENED), at("2026-09-22T14:29:00.000000000Z")),
            Err(SpecError::ClockWentBackwards)
        );
    }
}

/// The binding gate's snapshot is the risk state's, member for member, beside its mode; an
/// unreadable view gives none (DEC-2804 item 7).
#[test]
fn the_gates_risk_snapshot_is_the_risk_states() {
    let case = case("MC-R01");
    let (mandate, calendar) = (case_mandate(&case), nyse());
    let (fold, _) = mc_r01_journaled(&mandate, &calendar);
    let RiskView::Readable { snapshot, .. } = fold.view() else {
        panic!("readable");
    };
    let (risk, mode) = risk_snapshot(fold.view()).expect("a readable view has one");
    assert_eq!(mode, mandate_risk::AgentMode::Paused);
    assert_eq!(risk.agent_mode, mode);
    assert_eq!(risk.agent_equity, snapshot.agent_equity);
    assert_eq!(risk.high_water_mark, snapshot.high_water_mark);
    assert_eq!(risk.day_start_equity, snapshot.day_start_equity);
    assert_eq!(risk.capital_base, snapshot.capital_base);
    assert_eq!(risk.inherited_loss, snapshot.inherited_loss);
    assert_eq!(risk.active_rungs, snapshot.active_rungs);
    assert_eq!(risk.size_factor, snapshot.size_factor);
    let latched: Vec<String> = risk.latched.iter().map(|key| format!("{key:?}")).collect();
    let wanted: Vec<String> = snapshot
        .latched
        .iter()
        .map(|key| format!("{key:?}"))
        .collect();
    assert!(!wanted.is_empty(), "MC-R01 latches its rungs");
    assert_eq!(latched, wanted);
    for mode in [AgentMode::Normal, AgentMode::ExitsOnly, AgentMode::Stopped] {
        let (_, mapped) = risk_snapshot(&readable_in(mode)).unwrap();
        assert_eq!(format!("{mapped:?}"), format!("{mode:?}"));
    }
    let mut broken = open_case(&case, &mandate, &calendar);
    broken.append_failed();
    assert_eq!(risk_snapshot(broken.view()), None);
}

/// The deployed calendar (DEC-2804 item 6): an equity's sessions and regular-session seconds from
/// the exchange calendar, `overnight` while the market is closed; crypto's every second.
#[test]
fn the_deployed_calendar_answers_sessions_and_counts_seconds() {
    let equity = ExchangeRiskCalendar::new(AssetClass::UsEquity).unwrap();
    for (instant, session) in [
        ("2026-09-22T12:00:00.000000000Z", MarketSession::PreMarket),
        ("2026-09-22T14:30:00.000000000Z", MarketSession::Regular),
        ("2026-09-22T21:00:00.000000000Z", MarketSession::AfterHours),
        ("2026-09-26T15:00:00.000000000Z", MarketSession::Overnight),
    ] {
        assert_eq!(equity.session(at(instant)).unwrap(), session, "{instant}");
    }
    let day = (
        at("2026-09-22T00:00:00.000000000Z"),
        at("2026-09-23T00:00:00.000000000Z"),
    );
    assert_eq!(
        equity.seconds_between(day.0, day.1).unwrap(),
        23_400,
        "09:30 to 16:00 New York"
    );
    let crypto = ExchangeRiskCalendar::new(AssetClass::Crypto).unwrap();
    assert_eq!(crypto.seconds_between(day.0, day.1).unwrap(), 86_400);
    assert_eq!(
        crypto
            .session(at("2026-09-26T15:00:00.000000000Z"))
            .unwrap(),
        MarketSession::Crypto
    );
}

/// A tick in the opening's own second is the agent's and is stepped (DEC-2804 item 3): with no
/// breach time to confirm, a lifetime floor the inherited loss already breaches triggers at it.
#[test]
fn a_tick_in_the_opening_second_is_stepped() {
    let mandate = validated(
        "research_equity_pinned",
        &[serde_json::json!({"op": "replace", "path": "/risk/breach_confirm_s", "value": 0})],
    );
    let opening = Opening {
        inherited_loss_usd: Usd::parse("1500").unwrap(),
        ..opening()
    };
    let mut fold = AccountRiskFold::open(
        ACCOUNT,
        1,
        AGENT,
        &mandate,
        &opening,
        Some(XYZ),
        &Wall,
        TripwireState::default(),
    )
    .unwrap();
    let records = fold.fold(&tick(2, OPENED)).unwrap();
    let types: Vec<&str> = records.iter().map(|record| record.event_type).collect();
    assert!(
        types.contains(&"RiskLimitTriggered"),
        "the floor triggers at the opening second: {types:?}"
    );
}
