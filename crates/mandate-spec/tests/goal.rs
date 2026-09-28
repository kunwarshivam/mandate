//! Goal completion (`kind: goal`, MC-L01 to MC-L05; [§3.1]).
//!
//! §3.1 gives an `accumulate` goal three ways to finish and one way to keep running, and the reference
//! cases pin one instance of each. What a case set cannot pin is the arithmetic *between* its points:
//! that "remaining below one increment" is a comparison against the increment and not against zero,
//! that goal spend counts fees and is never reduced by a sale, and that the `end_date` boundary is the
//! New York midnight **after** the date rather than the date's own midnight.
//!
//! A `profit_stop` is deliberately not decided here (§3.1 confirms it by breach time in the risk state),
//! which is why [`GoalStatus::ConfirmedInRiskState`] exists and is asserted below: a caller that saw
//! `Running` would be invited to decide the same condition a second way.
//!
//! [§3.1]: ../../../docs/specs/mandate.md#31-goals-and-stop-conditions-dec-46-dec-59

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{arr, base, obj, s, with, with_all};
use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::{Price, Qty, Usd};
use mandate_spec::document::OnComplete;
use mandate_spec::document::ProvenanceMap;
use mandate_spec::goal::{GoalInputs, GoalStatus, status};
use mandate_spec::risk::{GoalReason, StopReason, ThenAction};
use mandate_spec::validate::{ValidatedMandate, ValidationContext};
use mandate_spec::{Mandate, SpecError};
use mandate_time::{Date, UtcNanos};

const GOAL_INSTRUMENT: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";

fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("a dollar amount")
}

fn context() -> ValidationContext {
    ValidationContext {
        account_equity_usd: usd("25000"),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-20").expect("a date"),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
    }
}

/// `btc_accumulator`'s goal on the builder's base: target 0.15, max spend 9000, end date 2026-12-31,
/// `on_complete: hold_protected`.
fn accumulator(changes: &[(&str, Option<Value>)]) -> ValidatedMandate {
    let mut all: Vec<(&str, Option<Value>)> = vec![
        ("/goal/type", Some(s("accumulate"))),
        ("/goal/instrument", Some(s(GOAL_INSTRUMENT))),
        ("/goal/target_qty", Some(s("0.15"))),
        ("/goal/max_avg_price", Some(s("58000"))),
        ("/goal/max_spend_usd", Some(s("9000"))),
        ("/goal/end_date", Some(s("2026-12-31"))),
        ("/goal/on_complete", Some(s("hold_protected"))),
        (
            "/universe/pinned_instruments",
            Some(arr(vec![obj(vec![
                ("asset_id", s(GOAL_INSTRUMENT)),
                ("symbol", s("AAA")),
                ("asset_class", s("us_equity")),
            ])])),
        ),
    ];
    all.extend_from_slice(changes);
    let document = Mandate::parse(&with_all(&all)).expect("the accumulate goal parses");
    ValidatedMandate::new(document, &context(), &[]).expect("a valid accumulate mandate")
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("an instant")
}

/// The four state figures every MC-L case carries, with the case's own values as defaults.
fn inputs(now: &str, position_qty: &str, goal_spent_usd: &str) -> GoalInputs {
    GoalInputs {
        now: at(now),
        position_qty: Qty::parse(position_qty).expect("a quantity"),
        goal_spent_usd: usd(goal_spent_usd),
        min_order_usd: usd("1"),
        qty_increment: Qty::parse("0.0001").expect("an increment"),
        ask: Price::parse("55000").expect("a price"),
    }
}

fn done(reason: GoalReason, on_complete: OnComplete, stop_reason: StopReason) -> GoalStatus {
    GoalStatus::Done {
        reason,
        then: ThenAction::Applied(on_complete),
        stop_reason,
    }
}

/// MC-L01 to MC-L05, each recomputed from §3.1's "done when" column rather than copied.
///
/// MC-L01: position 0.15 of a 0.15 target leaves 0, below the 0.0001 increment. MC-L02: 0.14995 leaves
/// 0.00005, still below it — the case's point being that fees paid in the asset leave a dust remainder
/// no order can clear. MC-L03: 8999.5 of 9000 spent leaves 0.5, below the 1.00 minimum order, while
/// 0.01 of quantity remains, so the reason is the spend and not the target. MC-L04 and MC-L05 straddle
/// 00:00 New York on 2027-01-01, the instant after the 2026-12-31 end date.
#[test]
#[ignore = "pending E6-4"]
fn the_five_reference_goals_finish_for_the_reason_section_three_one_gives() {
    let goal = accumulator(&[]);
    let expected_done = done(
        GoalReason::TargetQty,
        OnComplete::HoldProtected,
        StopReason::GoalComplete,
    );

    assert_eq!(
        status(
            &goal,
            &inputs("2026-09-21T15:00:00.000000000Z", "0.15", "8300")
        )
        .expect("a status"),
        expected_done,
        "MC-L01: nothing remains of the target"
    );
    assert_eq!(
        status(
            &goal,
            &inputs("2026-09-21T15:00:00.000000000Z", "0.14995", "8300")
        )
        .expect("a status"),
        expected_done,
        "MC-L02: 0.00005 remains, below one 0.0001 increment"
    );
    assert_eq!(
        status(
            &goal,
            &inputs("2026-09-21T15:00:00.000000000Z", "0.14", "8999.5")
        )
        .expect("a status"),
        done(
            GoalReason::MaxSpend,
            OnComplete::HoldProtected,
            StopReason::GoalComplete
        ),
        "MC-L03: 0.50 of spend remains, below the 1.00 minimum, while 0.01 of quantity still could be bought"
    );
    assert_eq!(
        status(
            &goal,
            &inputs("2027-01-01T05:00:00.000000000Z", "0.1", "5000")
        )
        .expect("a status"),
        done(
            GoalReason::EndDate,
            OnComplete::HoldProtected,
            StopReason::GoalComplete
        ),
        "MC-L04: 00:00 New York on the day after the end date"
    );
    assert_eq!(
        status(
            &goal,
            &inputs("2027-01-01T04:59:00.000000000Z", "0.1", "5000")
        )
        .expect("a status"),
        GoalStatus::Running,
        "MC-L05: 23:59 New York on the end date itself, still running"
    );
}

/// A remainder of exactly one increment is not below one increment, and neither is remaining spend of
/// exactly the minimum order.
///
/// Both boundaries are `<`, not `<=`: §3.1 says "below one increment" and "below the minimum order".
/// The reference cases sit either side of each boundary but on neither, so a `<=` would pass all five
/// while finishing every goal one order early.
#[test]
#[ignore = "pending E6-4"]
fn a_remainder_of_exactly_one_increment_or_one_minimum_order_is_still_running() {
    let goal = accumulator(&[]);
    assert_eq!(
        status(
            &goal,
            &inputs("2026-09-21T15:00:00.000000000Z", "0.1499", "8300")
        )
        .expect("a status"),
        GoalStatus::Running,
        "0.0001 remains, exactly one increment, so one more order can still be placed"
    );
    assert_eq!(
        status(
            &goal,
            &inputs("2026-09-21T15:00:00.000000000Z", "0.14", "8999")
        )
        .expect("a status"),
        GoalStatus::Running,
        "1.00 of spend remains, exactly the minimum order"
    );
}

/// When both the target and the spend are exhausted at once, the reason is `target_qty`.
///
/// §3.1 lists the quantity conditions before the spend one, and the journal carries exactly one reason,
/// so the order has to be settled somewhere; DEC-128 item 26 records it. No reference case has both
/// true, which is why this is stated here rather than left to whichever branch an implementation
/// happens to test first.
#[test]
#[ignore = "pending E6-4"]
fn the_target_is_the_reason_when_the_target_and_the_spend_run_out_together() {
    let goal = accumulator(&[]);
    assert_eq!(
        status(
            &goal,
            &inputs("2026-09-21T15:00:00.000000000Z", "0.15", "8999.5")
        )
        .expect("a status"),
        done(
            GoalReason::TargetQty,
            OnComplete::HoldProtected,
            StopReason::GoalComplete
        ),
        "the target is reached and the spend is exhausted; §3.1 lists the target first"
    );
}

/// The remaining quantity is also done when what is left is worth less than the minimum order.
///
/// §3.1 gives two quantity conditions, "below one increment" **or** "below the minimum order", and the
/// second needs the ask to evaluate. Here 0.0000001 BTC at 55000 is 0.0055, under the 1.00 minimum,
/// while the increment is set small enough that the first condition does not fire — so the goal can only
/// be done by the second, and only by reading the ask.
#[test]
#[ignore = "pending E6-4"]
fn a_remainder_worth_less_than_the_minimum_order_finishes_the_goal() {
    let goal = accumulator(&[]);
    let mut dust = inputs("2026-09-21T15:00:00.000000000Z", "0.1", "5000");
    dust.qty_increment = Qty::parse("0.000000001").expect("an increment");
    dust.position_qty = Qty::parse("0.1499999").expect("a quantity");
    assert_eq!(
        status(&goal, &dust).expect("a status"),
        done(
            GoalReason::TargetQty,
            OnComplete::HoldProtected,
            StopReason::GoalComplete
        ),
        "0.0000001 BTC at 55000 is 0.0055, below the 1.00 minimum order, though above one increment"
    );
}

/// `on_complete` is carried through, whichever of the three §3.1 values the owner chose.
///
/// MC-L01 to MC-L05 all use `hold_protected`, so the other two values are unpinned by the case set,
/// and MC-R16 and MC-R17 show what the risk state does with them.
#[test]
#[ignore = "pending E6-4"]
fn the_owners_on_complete_is_the_then() {
    for (value, on_complete) in [
        ("hold_protected", OnComplete::HoldProtected),
        ("disarm_ladder", OnComplete::DisarmLadder),
        ("release", OnComplete::Release),
    ] {
        let goal = accumulator(&[("/goal/on_complete", Some(s(value)))]);
        assert_eq!(
            status(
                &goal,
                &inputs("2026-09-21T15:00:00.000000000Z", "0.15", "8300")
            )
            .expect("a status"),
            done(GoalReason::TargetQty, on_complete, StopReason::GoalComplete),
            "on_complete {value} is what follows the goal"
        );
    }
}

/// A `continuous` goal finishes only on its `end_date`, and a null `end_date` never finishes.
#[test]
#[ignore = "pending E6-4"]
fn a_continuous_goal_ends_on_its_end_date_and_a_null_end_date_never_ends() {
    let dated = Mandate::parse(&with("/goal/end_date", Some(s("2026-12-31"))))
        .expect("a continuous goal with an end date");
    let dated = ValidatedMandate::new(dated, &context(), &[]).expect("valid");
    assert_eq!(
        status(&dated, &inputs("2027-01-01T05:00:00.000000000Z", "0", "0")).expect("a status"),
        done(
            GoalReason::EndDate,
            OnComplete::HoldProtected,
            StopReason::GoalComplete
        ),
        "the end date passes at 00:00 New York after it"
    );
    assert_eq!(
        status(&dated, &inputs("2027-01-01T04:59:59.000000000Z", "0", "0")).expect("a status"),
        GoalStatus::Running
    );

    let endless = Mandate::parse(&base()).expect("the base continuous goal");
    let endless = ValidatedMandate::new(endless, &context(), &[]).expect("valid");
    assert_eq!(
        status(
            &endless,
            &inputs("2099-01-01T05:00:00.000000000Z", "0", "0")
        )
        .expect("a status"),
        GoalStatus::Running,
        "a null end_date means no end (§3.1)"
    );
}

/// A `profit_stop` is never decided here.
///
/// §3.1 confirms it by breach time in the risk state (§5.6, MC-R22), so `status` says so rather than
/// answering `Running` and inviting a second, disagreeing answer. Its `end_date` is still this
/// function's, because a date needs no confirmation.
#[test]
#[ignore = "pending E6-4"]
fn a_profit_stop_is_the_risk_states_to_confirm_but_its_end_date_is_not() {
    let profit_stop = with(
        "/goal",
        Some(obj(vec![
            ("type", s("profit_stop")),
            ("profit_level", s("0.2")),
            ("end_date", s("2026-12-31")),
        ])),
    );
    let goal = Mandate::parse(&profit_stop).expect("a profit_stop goal");
    let goal = ValidatedMandate::new(goal, &context(), &[]).expect("valid");
    assert_eq!(
        status(&goal, &inputs("2026-09-21T15:00:00.000000000Z", "0.1", "0")).expect("a status"),
        GoalStatus::ConfirmedInRiskState,
        "§3.1 puts the profit level's confirmation in the risk state, not here"
    );
    assert_eq!(
        status(&goal, &inputs("2027-01-01T05:00:00.000000000Z", "0.1", "0")).expect("a status"),
        GoalStatus::Done {
            reason: GoalReason::EndDate,
            then: ThenAction::DiscretionaryExitAllThenRetire,
            stop_reason: StopReason::EndDate,
        },
        "an end_date needs no confirmation, and §3.1 gives a profit_stop no on_complete to apply"
    );
}

/// Out of arithmetic range is a typed error, never a silent "not done" (DEC-128 item 4).
///
/// Review round 1 was right that the first version of this test pinned nothing: its figures fit in every
/// type, and item 5's 256-bit intermediates do not overflow, so a correct implementation returned
/// `Running` and the test passed either way. The value that cannot be held is a **`target_qty` with more
/// places than `Qty`**: the schema admits 28 fractional digits on a `positive_decimal` and `Qty` holds
/// nine, so the document is schema-valid and the comparison §3.1 asks for cannot be made exactly. That is
/// item 4's case exactly — the mandate breaks no rule, and no rule defines an answer — so the only honest
/// result is `out_of_range` naming the path, never a goal quietly reported as running.
///
/// The name says `Qty` and not "no type", which round 2 was right to flag: `Usd` holds 28 places, so ten
/// places is only unholdable by the type §3.1's quantity comparison needs. A test name that overstates
/// what it proves is the very thing round 1 found here.
#[test]
#[ignore = "pending E6-4"]
fn a_target_quantity_a_qty_cannot_hold_is_out_of_range_and_never_a_running_goal() {
    let goal = accumulator(&[("/goal/target_qty", Some(s("0.1234567891")))]);
    let answer = status(
        &goal,
        &inputs("2026-09-21T15:00:00.000000000Z", "0.1", "5000"),
    );
    let Err(SpecError::OutOfRange { path, .. }) = &answer else {
        panic!(
            "a ten-place target_qty cannot be compared exactly against a nine-place Qty, so §3.1 has no answer: {answer:?}"
        );
    };
    assert_eq!(
        path.as_str(),
        "/goal/target_qty",
        "the error names the field that could not be held"
    );
}

/// A zero quantity increment is [`SpecError::InvalidInput`], not a guess (DEC-128 item 27).
///
/// Neither answer is safe, which is why it is an error rather than a default: nothing is ever below a
/// zero increment, so "not done" would keep buying forever, while "done" would stop a goal that is not
/// done. Review round 1 found item 27 introducing this rule with nothing exercising it.
#[test]
#[ignore = "pending E6-4"]
fn a_zero_quantity_increment_is_rejected_rather_than_guessed() {
    let goal = accumulator(&[]);
    let mut zero = inputs("2026-09-21T15:00:00.000000000Z", "0.1", "5000");
    zero.qty_increment = Qty::ZERO;
    let answer = status(&goal, &zero);
    assert!(
        matches!(
            &answer,
            Err(SpecError::InvalidInput { what }) if what.contains("increment")
        ),
        "a zero increment names itself rather than deciding the goal either way, got {answer:?}"
    );

    let mut smallest = inputs("2026-09-21T15:00:00.000000000Z", "0.1", "5000");
    smallest.qty_increment = Qty::parse("0.000000001").expect("one unit of the ninth place");
    assert!(
        status(&goal, &smallest).is_ok(),
        "the smallest increment a Qty can hold is usable, so only zero is rejected"
    );
}
