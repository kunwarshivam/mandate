//! Goal completion (`kind: goal`, MC-L01 to MC-L05; mandate spec §3.1), story E6-4.
//!
//! One test per row of §3.1's table and one per `on_complete`, because the table is where a goal type
//! and its outcome are joined and a reader cannot tell from the code alone which column was followed.
//! A `profit_stop` is deliberately **not** decided here: §3.1 confirms it by breach time in the risk
//! state, so `status` reports [`GoalStatus::ConfirmedInRiskState`] and `tests/risk.rs` carries it.

mod common;

use common::{instant, s, validated, with_all};
use mandate_canon::Value;
use mandate_num::{Price, Qty, ShareIncrement, Usd};
use mandate_spec::document::OnComplete;
use mandate_spec::goal::{GoalInputs, GoalStatus, status};
use mandate_spec::risk::{GoalReason, StopReason, ThenAction};

/// An accumulate goal on the two-stock base's risk block: one pinned instrument (V-003), a quantity
/// target, and a spend cap, which is the shape every MC-L case uses.
fn accumulate(on_complete: &str, end_date: Option<&str>) -> Value {
    with_all(&[
        ("/universe/max_instruments", Some(common::i(1))),
        ("/universe/pinned_instruments/1", None),
        (
            "/goal",
            Some(common::obj(vec![
                ("type", s("accumulate")),
                ("instrument", s(common::ASSET_A)),
                ("target_qty", s("0.15")),
                ("max_avg_price", s("58000")),
                ("max_spend_usd", s("9000")),
                ("end_date", end_date.map_or(Value::Null, s)),
                ("on_complete", s(on_complete)),
            ])),
        ),
    ])
}

fn inputs(now: &str, position_qty: &str, spent: &str) -> GoalInputs {
    GoalInputs {
        now: instant(now),
        position_qty: Qty::parse(position_qty).expect("a quantity"),
        goal_spent_usd: Usd::parse(spent).expect("a dollar amount"),
        min_order_usd: Usd::parse("1").expect("a dollar amount"),
        qty_increment: ShareIncrement::Fractional,
        ask: Price::parse("55000").expect("a price"),
    }
}

fn done(mandate: &Value, inputs: &GoalInputs) -> (GoalReason, ThenAction, StopReason) {
    match status(&validated(mandate), inputs).unwrap_or_else(|e| panic!("evaluable: {e}")) {
        GoalStatus::Done {
            reason,
            then,
            stop_reason,
        } => (reason, then, stop_reason),
        other => panic!("the goal should be done, got {other:?}"),
    }
}

/// §3.1's `accumulate` row: the quantity target, the spend cap, and the end date each complete the
/// goal, each with its own reason, and none of them completes it early.
#[test]
#[ignore = "pending E6-4"]
fn one_test_per_row_of_the_goal_table() {
    let goal = accumulate("hold_protected", Some("2026-12-31"));
    let running = status(
        &validated(&goal),
        &inputs("2026-09-21T15:00:00.000000000Z", "0.1", "5000"),
    )
    .unwrap_or_else(|e| panic!("evaluable: {e}"));
    assert_eq!(
        running,
        GoalStatus::Running,
        "0.05 BTC and $4000 of room left is not a completed goal"
    );

    let (reason, then, stop) = done(
        &goal,
        &inputs("2026-09-21T15:00:00.000000000Z", "0.15", "8300"),
    );
    assert_eq!(reason, GoalReason::TargetQty, "the target quantity is met");
    assert_eq!(then, ThenAction::Applied(OnComplete::HoldProtected));
    assert_eq!(stop, StopReason::GoalComplete);

    let (reason, _, _) = done(
        &goal,
        &inputs("2026-09-21T15:00:00.000000000Z", "0.14", "8999.5"),
    );
    assert_eq!(
        reason,
        GoalReason::MaxSpend,
        "$0.50 of the $9000 cap is below the $1 minimum order"
    );

    let (reason, _, _) = done(
        &goal,
        &inputs("2027-01-01T05:00:00.000000000Z", "0.1", "5000"),
    );
    assert_eq!(
        reason,
        GoalReason::EndDate,
        "00:00 New York on 2027-01-01 is after the 2026-12-31 end date"
    );
}

/// §3.1: the end date is the goal's **last** risk day, so 23:59 New York on it is still inside the
/// goal and one minute later is not.
#[test]
#[ignore = "pending E6-4"]
fn the_end_date_is_the_last_risk_day_of_the_goal() {
    let goal = accumulate("hold_protected", Some("2026-12-31"));
    let still_running = status(
        &validated(&goal),
        &inputs("2027-01-01T04:59:00.000000000Z", "0.1", "5000"),
    )
    .unwrap_or_else(|e| panic!("evaluable: {e}"));
    assert_eq!(
        still_running,
        GoalStatus::Running,
        "23:59 New York on the end date is inside the goal's last risk day"
    );
    let (reason, _, _) = done(
        &goal,
        &inputs("2027-01-01T05:00:00.000000000Z", "0.1", "5000"),
    );
    assert_eq!(
        reason,
        GoalReason::EndDate,
        "one minute later it has passed"
    );
}

/// §3.1's `on_complete` table: the chosen value is what `then` carries, whichever reason completed the
/// goal. A rule that hard-coded `hold_protected` would pass MC-L01 to MC-L05 and be wrong for the
/// other two bases.
#[test]
#[ignore = "pending E6-4"]
fn each_on_complete_is_carried_into_then() {
    for (chosen, expected) in [
        ("hold_protected", OnComplete::HoldProtected),
        ("disarm_ladder", OnComplete::DisarmLadder),
        ("release", OnComplete::Release),
    ] {
        let goal = accumulate(chosen, Some("2026-12-31"));
        let (_, then, stop) = done(
            &goal,
            &inputs("2026-09-21T15:00:00.000000000Z", "0.15", "8300"),
        );
        assert_eq!(
            then,
            ThenAction::Applied(expected),
            "`on_complete: {chosen}` is what follows the goal"
        );
        assert_eq!(stop, StopReason::GoalComplete);
    }
}

/// §3.1: a `continuous` goal ends only on its end date, and a null end date never ends it.
#[test]
#[ignore = "pending E6-4"]
fn a_continuous_goal_ends_only_on_its_end_date() {
    let never = with_all(&[("/goal/end_date", Some(Value::Null))]);
    assert_eq!(
        status(
            &validated(&never),
            &inputs("2030-01-01T05:00:00.000000000Z", "0", "0")
        )
        .unwrap_or_else(|e| panic!("evaluable: {e}")),
        GoalStatus::Running,
        "`end_date: null` means no end (§3.1)"
    );
    let ends = with_all(&[("/goal/end_date", Some(s("2026-12-31")))]);
    let (reason, then, stop) = done(&ends, &inputs("2027-01-01T05:00:00.000000000Z", "0", "0"));
    assert_eq!(reason, GoalReason::EndDate);
    assert_eq!(then, ThenAction::Applied(OnComplete::HoldProtected));
    assert_eq!(stop, StopReason::GoalComplete);
}

/// §3.1: a `profit_stop` is confirmed in the risk state, so the goal rule refuses to decide it. A
/// `Running` here would invite a caller to answer the same condition twice, in two places.
#[test]
#[ignore = "pending E6-4"]
fn a_profit_stop_is_confirmed_in_the_risk_state() {
    let goal = with_all(&[(
        "/goal",
        Some(common::obj(vec![
            ("type", s("profit_stop")),
            ("profit_level", s("0.2")),
            ("end_date", Value::Null),
        ])),
    )]);
    assert_eq!(
        status(
            &validated(&goal),
            &inputs("2026-09-21T15:00:00.000000000Z", "0.1", "5000")
        )
        .unwrap_or_else(|e| panic!("evaluable: {e}")),
        GoalStatus::ConfirmedInRiskState,
        "§3.1 confirms it by breach time in §5.6, not here"
    );
}
