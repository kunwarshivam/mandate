//! E6-15 slice B1 (DEC-2301 item 3, DEC-2304): `risk_state::AccountRiskFold` reads the account
//! stream's rows into a risk view, and a row it cannot read makes the view unreadable for good. An
//! unreadable view holds new openings and no exit, protective order, risk exit, or kill switch
//! (`AGENTS.md` rules 3 and 13). Until B2 fills in `fold`, every test that folds a row fails at its
//! `Unimplemented` stub.

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
use mandate_time::{Date, UtcNanos};
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

/// `research_equity_pinned`, the reference bases' one pinned US equity, validated.
fn mandate() -> ValidatedMandate {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json")).unwrap();
    let document = fixture
        .pointer("/bases/research_equity_pinned/mandate")
        .unwrap();
    let canonical = parse(&serde_json::to_vec(document).unwrap()).unwrap();
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
