//! The risk state (`kind: risk_state`, MC-R01 to MC-R24; mandate spec §5), story E6-4.
//!
//! The reference cases carry the arithmetic. These carry what a case list cannot: the order §5.2
//! evaluates in, the rules a careless fold gets backwards (a bounce that restarts confirmation, a
//! second quote a hard trigger never waits for, a withdrawal that shrinks the loss carry), and the
//! refusals that must still come back as an outcome rather than an `Err`.
//!
//! Every test here is `#[ignore = "pending E6-4"]` and fails on this stream's stubs (DEC-110).

mod common;

use common::{NyseRegularSeconds, base, fraction, i, instant, ladder_rung, s, validated, with_all};
use mandate_canon::Value;
use mandate_domain::{AgentMode, AssetClass, AssetId, MarketSession, Side};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_spec::document::LadderAction;
use mandate_spec::risk::{
    Confirmation, GoalReason, Input, InstrumentRestriction, Latch, LiftReason, LimitKey, Opening,
    Outcome, Rejection, RemovalReason, Restriction, RiskEvent, RiskState, Step, StopReason,
    ThenAction, TriggerReason, UniverseChange, reset_lift_order, size_factor,
};

const START: &str = "2026-09-21T14:00:00.000000000Z";
const OTHER_INSTRUMENT: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";

/// The two-stock swing base with the position limits every `risk_state` case widens, so the opening
/// position is the whole allocation and agent equity is exactly 100 × the bid.
fn swing(changes: &[(&str, Option<Value>)]) -> Value {
    let mut all: Vec<(&str, Option<Value>)> = vec![
        ("/risk/max_position_usd", Some(s("10000"))),
        ("/risk/max_position_fraction", Some(s("1"))),
        ("/risk/max_gross_exposure_usd", Some(s("10000"))),
    ];
    all.extend_from_slice(changes);
    with_all(&all)
}

fn opening() -> Opening {
    Opening {
        position_qty: Qty::parse("100").expect("a quantity"),
        avg_cost: Price::parse("100").expect("a price"),
        asset_class: AssetClass::UsEquity,
        at: instant(START),
        inherited_loss_usd: Usd::ZERO,
        mark_max_age_s: 120,
    }
}

fn open(document: &Value) -> RiskState {
    RiskState::open(&validated(document), &opening(), &NyseRegularSeconds).expect("the state opens")
}

fn mark(at: &str, bid: &str) -> Step {
    marked(at, bid, MarketSession::Regular, true)
}

fn marked(at: &str, bid: &str, session: MarketSession, sane: bool) -> Step {
    Step {
        at: instant(at),
        session,
        input: Input::Mark {
            bid: Price::parse(bid).expect("a price"),
            sane,
        },
    }
}

fn at_regular(at: &str, input: Input) -> Step {
    Step {
        at: instant(at),
        session: MarketSession::Regular,
        input,
    }
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("a dollar amount")
}

/// Applies each step in order and returns every outcome, so a test can read the one it is about
/// without losing the ones before it.
fn fold(state: &mut RiskState, steps: &[Step]) -> Vec<Outcome> {
    steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            state
                .step(step)
                .unwrap_or_else(|e| panic!("step {} was refused with an error: {e}", index + 1))
        })
        .collect()
}

fn triggered(outcome: &Outcome) -> Vec<(LimitKey, Option<TriggerReason>)> {
    outcome
        .journal
        .iter()
        .filter_map(|event| match event {
            RiskEvent::RiskLimitTriggered { limit, reason, .. } => Some((*limit, *reason)),
            _ => None,
        })
        .collect()
}

fn lifted(outcome: &Outcome) -> Vec<(LimitKey, Option<LiftReason>)> {
    outcome
        .journal
        .iter()
        .filter_map(|event| match event {
            RiskEvent::RiskLimitLifted { limit, reason, .. } => Some((*limit, *reason)),
            _ => None,
        })
        .collect()
}

/// §5.2: rungs in ascending `at`, then the daily loss, then the effective mode; the journal follows
/// that order. Asserted as the order itself rather than as one expected list, because the claim the
/// spec makes is about the sequence, not about which limits a particular price happens to trip.
#[test]
#[ignore = "pending E6-4"]
fn journal_order_follows_the_evaluation_order() {
    let document = swing(&[("/risk/breach_confirm_s", Some(i(0)))]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "96.6"),
        ],
    );
    let drop = outcomes.last().expect("two steps");
    let order = triggered(drop);
    let rungs: Vec<u8> = order
        .iter()
        .filter_map(|(limit, _)| match limit {
            LimitKey::DrawdownRung(index) => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(
        rungs,
        vec![0, 1, 2],
        "H = 10500 and E = 9660 puts drawdown at 0.08, so all three rungs fire, in ascending `at`"
    );
    let daily = order
        .iter()
        .position(|(limit, _)| *limit == LimitKey::MaxDailyLoss)
        .expect("E - E0 = -340 breaches a 2% daily loss on E0 = 10000");
    assert_eq!(
        daily,
        order.len() - 1,
        "the daily loss is evaluated after every rung"
    );
    assert!(
        matches!(
            drop.journal.last(),
            Some(RiskEvent::AgentModeApplied { .. })
        ),
        "the effective mode is last, got: {:?}",
        drop.journal
    );
}

/// MI-6: the effective mode is the strictest restriction, and one change journals one event however
/// many restrictions arrived with it.
#[test]
#[ignore = "pending E6-4"]
fn the_strictest_restriction_wins() {
    let document = swing(&[("/risk/breach_confirm_s", Some(i(0)))]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "96.6"),
        ],
    );
    let drop = outcomes.last().expect("two steps");
    assert!(
        drop.snapshot.restrictions.contains(&Restriction::DailyLoss)
            && drop
                .snapshot
                .restrictions
                .contains(&Restriction::DrawdownExitsOnly)
            && drop
                .snapshot
                .restrictions
                .contains(&Restriction::DrawdownFlatten),
        "three restrictions arrive together, got: {:?}",
        drop.snapshot.restrictions
    );
    assert_eq!(
        drop.snapshot.agent_mode,
        AgentMode::Paused,
        "`drawdown_flatten` is `paused`, which is stricter than the two `exits_only` restrictions"
    );
    let modes: Vec<(AgentMode, AgentMode)> = drop
        .journal
        .iter()
        .filter_map(|event| match event {
            RiskEvent::AgentModeApplied { from, to } => Some((*from, *to)),
            _ => None,
        })
        .collect();
    assert_eq!(
        modes,
        vec![(AgentMode::Normal, AgentMode::Paused)],
        "one event for one change, never one per restriction"
    );
}

/// §5.6: breach time accumulates over every interval that **starts** in breach, and resets only after
/// the condition has been false continuously for `breach_confirm_s`.
///
/// Stated against [`Confirmation`] itself rather than through a price series, so the accumulator is
/// pinned without the equity fold in the way. This is MC-R02's timeline and planted bug 1.
#[test]
#[ignore = "pending E6-4"]
fn a_short_recovery_does_not_restart_confirmation() {
    let mut confirm = Confirmation::default();
    assert!(
        !confirm.update(true, 0, 60),
        "the first breaching input starts the clock at zero"
    );
    assert!(!confirm.update(true, 30, 60));
    assert_eq!(confirm.accumulated_s(), 30);
    assert!(
        !confirm.update(false, 15, 60),
        "the interval started in breach, so it counts even though this input is not"
    );
    assert_eq!(confirm.accumulated_s(), 45);
    assert!(!confirm.update(true, 5, 60));
    assert_eq!(
        confirm.accumulated_s(),
        45,
        "a 5 s recovery is far short of the 60 s the reset needs, so nothing is lost"
    );
    assert!(
        confirm.update(true, 20, 60),
        "45 + 20 = 65 >= 60, so the limit triggers here"
    );
}

/// §5.6's other half: a recovery **at least** `breach_confirm_s` long does reset, and confirmation
/// then starts afresh rather than from where it left off.
#[test]
#[ignore = "pending E6-4"]
fn a_recovery_longer_than_the_window_restarts_it() {
    let mut confirm = Confirmation::default();
    assert!(!confirm.update(true, 0, 60));
    assert!(!confirm.update(true, 45, 60));
    assert_eq!(confirm.accumulated_s(), 45);
    assert!(!confirm.update(false, 0, 60));
    assert!(!confirm.update(false, 60, 60));
    assert_eq!(
        confirm.accumulated_s(),
        0,
        "60 s of continuous falsity is the reset"
    );
    assert!(!confirm.is_pending());
    assert!(!confirm.update(true, 0, 60));
    assert!(
        !confirm.update(true, 59, 60),
        "confirmation starts afresh, so 59 s is not yet enough"
    );
    assert!(confirm.update(true, 1, 60));
}

/// MI-4, DEC-63: one bad print applies `hard_breach` at once and latches nothing. A sane quote below
/// the hard level clears it, carrying `hard_breach_cleared` and no action (DEC-128 item 15).
#[test]
#[ignore = "pending E6-4"]
fn a_single_flash_print_latches_nothing() {
    let document = swing(&[("/risk/max_daily_loss", Some(s("0.5")))]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "87"),
            mark("2026-09-21T14:01:05.000000000Z", "103"),
        ],
    );
    let flash = outcomes.first().expect("the flash print");
    assert!(
        flash
            .snapshot
            .restrictions
            .contains(&Restriction::HardBreach),
        "H - E = 1300 is past 1.25 x 0.08 x 10000 = 1000, so `hard_breach` applies at once"
    );
    assert_eq!(flash.snapshot.agent_mode, AgentMode::ExitsOnly);
    assert!(
        flash.snapshot.latched.is_empty(),
        "nothing latches on one quote, got: {:?}",
        flash.snapshot.latched
    );
    assert!(
        triggered(flash)
            .iter()
            .any(|(_, reason)| *reason == Some(TriggerReason::HardBreachPending)),
        "the escalation is journalled as pending, got: {:?}",
        flash.journal
    );
    let recovery = outcomes.last().expect("the recovery quote");
    assert!(
        !recovery
            .snapshot
            .restrictions
            .contains(&Restriction::HardBreach),
        "a sane quote below the hard level clears it"
    );
    assert!(
        lifted(recovery)
            .iter()
            .any(|(_, reason)| *reason == Some(LiftReason::HardBreachCleared)),
        "the lift carries `hard_breach_cleared` instead of an action, got: {:?}",
        recovery.journal
    );
}

/// DEC-63's second quote: the hard level holding again at least min(`breach_confirm_s`, 10) s later
/// is what latches the rung, with reason `hard_trigger` and the agent-scoped kill switch.
#[test]
#[ignore = "pending E6-4"]
fn a_flatten_hard_trigger_needs_a_second_quote() {
    let document = swing(&[("/risk/max_daily_loss", Some(s("0.5")))]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "87"),
            mark("2026-09-21T14:01:05.000000000Z", "87"),
            mark("2026-09-21T14:01:20.000000000Z", "87"),
        ],
    );
    let too_soon = outcomes.get(1).expect("the five-second repeat");
    assert!(
        too_soon.snapshot.latched.is_empty(),
        "5 s is short of the 10 s wait, so still nothing is latched"
    );
    let second = outcomes.last().expect("the second sane quote");
    assert!(
        second.snapshot.latched.contains(&LimitKey::DrawdownRung(2)),
        "20 s after the first print the hard level still holds, so the rung latches"
    );
    assert!(
        triggered(second)
            .iter()
            .any(|(limit, reason)| *limit == LimitKey::DrawdownRung(2)
                && *reason == Some(TriggerReason::HardTrigger)),
        "the latch names the hard trigger, got: {:?}",
        second.journal
    );
    assert!(
        second.journal.iter().any(|event| matches!(
            event,
            RiskEvent::KillSwitchActivated {
                initiator: LimitKey::DrawdownRung(2),
                ..
            }
        )),
        "a `flatten_and_pause` rung activates the agent-scoped kill switch"
    );
    assert_eq!(second.snapshot.agent_mode, AgentMode::Paused);
}

/// §5.4: a breach still confirming at the rollover keeps confirming against the **previous** day's
/// E₀, and latches with reason `resolved_at_rollover`.
#[test]
#[ignore = "pending E6-4"]
fn a_pending_breach_resolves_at_the_rollover() {
    let document = swing(&[("/risk/breach_confirm_s", Some(i(300)))]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T19:59:00.000000000Z", "97"),
            at_regular("2026-09-21T20:03:00.000000000Z", Input::Clock),
            Step {
                at: instant("2026-09-22T04:00:00.000000000Z"),
                session: MarketSession::Overnight,
                input: Input::RiskDayStarted,
            },
        ],
    );
    let waiting = outcomes.get(1).expect("the clock tick");
    assert!(
        waiting.pending.contains(&LimitKey::MaxDailyLoss),
        "240 s of a 300 s window: still confirming, got pending {:?}",
        waiting.pending
    );
    assert!(waiting.snapshot.latched.is_empty());
    let rollover = outcomes.last().expect("the rollover");
    assert!(
        triggered(rollover)
            .iter()
            .any(|(limit, reason)| *limit == LimitKey::MaxDailyLoss
                && *reason == Some(TriggerReason::ResolvedAtRollover)),
        "E - E0 = -300 against the day it started in, not against the new E0, got: {:?}",
        rollover.journal
    );
    assert!(
        rollover.snapshot.latched.contains(&LimitKey::MaxDailyLoss),
        "a breach that confirms at the rollover latches"
    );
}

/// §5.4's other half: a flash print just before midnight whose condition then stays false for
/// `breach_confirm_s` is discarded, so it latches nothing at the rollover.
#[test]
#[ignore = "pending E6-4"]
fn a_flash_breach_before_midnight_is_discarded() {
    let document = swing(&[("/risk/breach_confirm_s", Some(i(300)))]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T19:59:00.000000000Z", "97"),
            mark("2026-09-21T19:59:30.000000000Z", "99"),
            at_regular("2026-09-21T20:05:00.000000000Z", Input::Clock),
            Step {
                at: instant("2026-09-22T04:00:00.000000000Z"),
                session: MarketSession::Overnight,
                input: Input::RiskDayStarted,
            },
        ],
    );
    for (index, outcome) in outcomes.iter().enumerate() {
        assert!(
            !outcome.snapshot.latched.contains(&LimitKey::MaxDailyLoss),
            "step {}: one print 30 s long never latches a daily loss",
            index + 1
        );
    }
    let rollover = outcomes.last().expect("the rollover");
    assert!(
        !rollover.pending.contains(&LimitKey::MaxDailyLoss),
        "330 s of falsity is past the 300 s reset, so nothing is still confirming"
    );
    assert!(
        triggered(rollover).is_empty(),
        "nothing triggers at the rollover, got: {:?}",
        rollover.journal
    );
}

/// §5.4: the lift needs a new risk day **and** `daily_breach_min_s` since the breach. The new day
/// alone is not enough.
#[test]
#[ignore = "pending E6-4"]
fn the_daily_lift_waits_for_the_minimum() {
    let document = swing(&[
        ("/risk/breach_confirm_s", Some(i(0))),
        ("/risk/daily_breach_min_s", Some(i(36000))),
    ]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T19:59:00.000000000Z", "97"),
            Step {
                at: instant("2026-09-22T04:00:00.000000000Z"),
                session: MarketSession::Overnight,
                input: Input::RiskDayStarted,
            },
            Step {
                at: instant("2026-09-22T06:00:00.000000000Z"),
                session: MarketSession::Overnight,
                input: Input::Clock,
            },
        ],
    );
    let rollover = outcomes.get(1).expect("the rollover");
    assert!(
        rollover.snapshot.latched.contains(&LimitKey::MaxDailyLoss),
        "28 920 s since the breach is short of 36 000, so the new day alone lifts nothing"
    );
    let later = outcomes.last().expect("the later tick");
    assert!(
        !later.snapshot.latched.contains(&LimitKey::MaxDailyLoss),
        "36 060 s past the breach, in a new risk day, is the lift"
    );
    assert!(
        lifted(later)
            .iter()
            .any(|(limit, _)| *limit == LimitKey::MaxDailyLoss),
        "the lift is journalled, got: {:?}",
        later.journal
    );
}

/// §5.4: during the lift delay the new day's loss is measured against the **new** E₀, and a confirmed
/// new-day breach renews the latch with reason `new_day_breach`.
#[test]
#[ignore = "pending E6-4"]
fn a_new_day_breach_renews_the_latch() {
    let document = swing(&[
        ("/risk/breach_confirm_s", Some(i(0))),
        ("/risk/daily_breach_min_s", Some(i(86400))),
    ]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T19:59:00.000000000Z", "97"),
            Step {
                at: instant("2026-09-22T04:00:00.000000000Z"),
                session: MarketSession::Overnight,
                input: Input::RiskDayStarted,
            },
            mark("2026-09-22T14:00:00.000000000Z", "95"),
        ],
    );
    let rollover = outcomes.get(1).expect("the rollover");
    assert_eq!(
        rollover.snapshot.day_start_equity,
        usd("9700"),
        "E0 becomes E at 00:00 New York"
    );
    let new_day = outcomes.last().expect("the new day's mark");
    assert!(
        triggered(new_day)
            .iter()
            .any(|(limit, reason)| *limit == LimitKey::MaxDailyLoss
                && *reason == Some(TriggerReason::NewDayBreach)),
        "E - E0 = -200 against the new E0 of 9700 is past 2%, so the latch renews, got: {:?}",
        new_day.journal
    );
}

/// §5.4: a `flatten_and_pause` daily loss becomes `exits_only` once the agent is flat and the owner
/// acknowledges — the restriction stays, its mode softens.
#[test]
#[ignore = "pending E6-4"]
fn a_daily_flatten_acknowledged_after_flat_leaves_exits_only() {
    let document = swing(&[
        ("/risk/breach_confirm_s", Some(i(0))),
        ("/risk/daily_loss_action", Some(s("flatten_and_pause"))),
        ("/risk/max_drawdown", Some(s("0.5"))),
        (
            "/risk/drawdown_ladder",
            Some(Value::Array(vec![rung_value(
                "0.4",
                "scale_sizes",
                Some("0.5"),
            )])),
        ),
    ]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "97"),
            at_regular(
                "2026-09-21T14:02:00.000000000Z",
                Input::Fill {
                    side: Side::Sell,
                    qty: Qty::parse("100").expect("a quantity"),
                    price: Price::parse("97").expect("a price"),
                },
            ),
            at_regular(
                "2026-09-21T14:03:00.000000000Z",
                Input::OwnerAcknowledged {
                    restriction: Latch::DailyLoss,
                },
            ),
        ],
    );
    let breach = outcomes.first().expect("the breach");
    assert_eq!(
        breach.snapshot.agent_mode,
        AgentMode::Paused,
        "`flatten_and_pause` pauses the agent"
    );
    let acknowledged = outcomes.last().expect("the acknowledgment");
    assert_eq!(acknowledged.rejection, None, "the agent is flat");
    assert!(
        acknowledged
            .snapshot
            .restrictions
            .contains(&Restriction::DailyLoss),
        "acknowledging softens the mode; the restriction lifts on its own schedule"
    );
    assert_eq!(acknowledged.snapshot.agent_mode, AgentMode::ExitsOnly);
}

/// §5.8: acknowledging the ladder while a flatten has not finished is refused, and the refusal comes
/// back as an outcome rather than an `Err` (DEC-128 item 16).
#[test]
#[ignore = "pending E6-4"]
fn acknowledgment_is_rejected_while_flattening() {
    let document = swing(&[
        ("/risk/breach_confirm_s", Some(i(0))),
        ("/risk/max_daily_loss", Some(s("0.5"))),
    ]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "91"),
            at_regular(
                "2026-09-21T14:02:00.000000000Z",
                Input::OwnerAcknowledged {
                    restriction: Latch::DrawdownLadder,
                },
            ),
        ],
    );
    let refused = outcomes.last().expect("the acknowledgment");
    assert_eq!(
        refused.rejection,
        Some(Rejection::FlattenInProgress),
        "100 shares are still held, so the flatten has not finished"
    );
    assert!(
        refused
            .snapshot
            .latched
            .contains(&LimitKey::DrawdownRung(2)),
        "a refused acknowledgment changes nothing"
    );
    assert!(
        !refused
            .journal
            .iter()
            .any(|event| matches!(event, RiskEvent::HighWaterMarkReset { .. })),
        "no reset happens, got: {:?}",
        refused.journal
    );
}

/// §5.7: the lifetime floor is the one latch the owner cannot acknowledge.
#[test]
#[ignore = "pending E6-4"]
fn the_floor_cannot_be_acknowledged() {
    let mut state = open(&floor_document());
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "89"),
            at_regular(
                "2026-09-21T14:02:00.000000000Z",
                Input::OwnerAcknowledged {
                    restriction: Latch::LifetimeFloor,
                },
            ),
        ],
    );
    let breach = outcomes.first().expect("the breach");
    assert!(
        breach.snapshot.latched.contains(&LimitKey::LifetimeFloor),
        "E = 8900 is at or below C x (1 - 0.1) + L = 9000"
    );
    let refused = outcomes.last().expect("the acknowledgment");
    assert_eq!(refused.rejection, Some(Rejection::NotAcknowledgeable));
    assert!(
        refused.snapshot.latched.contains(&LimitKey::LifetimeFloor),
        "the floor stays latched"
    );
}

/// §5.7: in a single-user workspace a loosening version applies only once the first full risk day
/// after the confirmation day has ended; earlier it is `waiting_period`.
#[test]
#[ignore = "pending E6-4"]
fn loosening_the_floor_waits_a_full_risk_day() {
    let mut state = open(&floor_document());
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "89"),
            at_regular(
                "2026-09-21T14:02:00.000000000Z",
                Input::FloorLoosened {
                    new_max_loss_from_allocation: fraction("0.2"),
                    confirmed_at: instant("2026-09-21T14:02:00.000000000Z"),
                    independent_approval: false,
                },
            ),
        ],
    );
    let too_soon = outcomes.last().expect("the loosening");
    assert_eq!(
        too_soon.rejection,
        Some(Rejection::WaitingPeriod),
        "the confirmation day has not even ended"
    );
    assert!(too_soon.snapshot.latched.contains(&LimitKey::LifetimeFloor));
}

/// §5.7: a version that does not loosen, and one that leaves equity at or below the new floor, are
/// both refused — the floor lifts only on a strict improvement.
#[test]
#[ignore = "pending E6-4"]
fn a_version_that_leaves_equity_below_the_new_floor_is_rejected() {
    let mut state = open(&floor_document());
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "89"),
            at_regular(
                "2026-09-21T14:02:00.000000000Z",
                Input::FloorLoosened {
                    new_max_loss_from_allocation: fraction("0.05"),
                    confirmed_at: instant("2026-09-20T14:00:00.000000000Z"),
                    independent_approval: true,
                },
            ),
            at_regular(
                "2026-09-21T14:03:00.000000000Z",
                Input::FloorLoosened {
                    new_max_loss_from_allocation: fraction("0.11"),
                    confirmed_at: instant("2026-09-19T14:00:00.000000000Z"),
                    independent_approval: true,
                },
            ),
        ],
    );
    assert_eq!(
        outcomes.get(1).expect("the tightening").rejection,
        Some(Rejection::NotLoosening),
        "0.05 is below the mandate's 0.1, so it is not a loosening at all"
    );
    assert_eq!(
        outcomes.last().expect("the marginal loosening").rejection,
        Some(Rejection::StillBelowNewFloor),
        "E = 8900 is exactly C x (1 - 0.11) + L, and the lift needs a strict improvement"
    );
}

/// MI-14: the loss carry is max(0, net contributed − E), so withdrawing before retiring cannot shrink
/// it. Asserted differentially — the same market, once with a withdrawal and once without — so the
/// oracle is the other run rather than a number typed in beside the rule.
#[test]
#[ignore = "pending E6-4"]
fn a_withdrawal_cannot_shrink_the_loss_carry() {
    let document = swing(&[
        ("/risk/breach_confirm_s", Some(i(0))),
        ("/risk/max_daily_loss", Some(s("0.5"))),
    ]);
    let sell = |at: &str| {
        at_regular(
            at,
            Input::Fill {
                side: Side::Sell,
                qty: Qty::parse("100").expect("a quantity"),
                price: Price::parse("91.5").expect("a price"),
            },
        )
    };
    let stop = |at: &str| {
        at_regular(
            at,
            Input::AgentStopped {
                reason: StopReason::OwnerStop,
            },
        )
    };
    let mut kept = open(&document);
    let without = fold(
        &mut kept,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "91.5"),
            sell("2026-09-21T14:02:00.000000000Z"),
            stop("2026-09-21T14:03:00.000000000Z"),
        ],
    );
    let mut withdrawn = open(&document);
    let with = fold(
        &mut withdrawn,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "91.5"),
            sell("2026-09-21T14:02:00.000000000Z"),
            at_regular(
                "2026-09-21T14:03:00.000000000Z",
                Input::AllocationChange {
                    delta_usd: usd("-9000"),
                },
            ),
            stop("2026-09-21T14:04:00.000000000Z"),
        ],
    );
    let carry = |outcomes: &[Outcome]| {
        outcomes
            .last()
            .expect("the stop")
            .journal
            .iter()
            .find_map(|event| match event {
                RiskEvent::AgentStopped { loss_carry_usd, .. } => Some(*loss_carry_usd),
                _ => None,
            })
            .expect("`AgentStopped` carries the loss")
    };
    assert_eq!(
        carry(&without),
        usd("850"),
        "net contributed 10000 against equity 9150"
    );
    assert_eq!(
        carry(&with),
        carry(&without),
        "the withdrawal lowered N and E equally, so the carry is the same number"
    );
}

/// §5.1 rule 1: an allocation increase is refused while any limit is latched, and a decrease that
/// would leave equity below the agent's exposure is refused too.
#[test]
#[ignore = "pending E6-4"]
fn an_increase_is_rejected_while_latched() {
    let document = swing(&[
        ("/risk/breach_confirm_s", Some(i(0))),
        ("/risk/max_daily_loss", Some(s("0.5"))),
    ]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "91"),
            at_regular(
                "2026-09-21T14:02:00.000000000Z",
                Input::AllocationChange {
                    delta_usd: usd("1000"),
                },
            ),
        ],
    );
    let refused = outcomes.last().expect("the increase");
    assert_eq!(
        refused.rejection,
        Some(Rejection::IncreaseBlockedWhileLatched)
    );
    assert_eq!(
        refused.snapshot.net_contributed,
        usd("10000"),
        "a refused change contributes nothing"
    );
}

/// §5.1 rule 1: E + Δ below the agent's gross exposure is `equity_below_exposure`.
#[test]
#[ignore = "pending E6-4"]
fn a_decrease_below_exposure_is_rejected() {
    let document = swing(&[("/risk/max_daily_loss", Some(s("0.5")))]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[at_regular(
            "2026-09-21T14:01:00.000000000Z",
            Input::AllocationChange {
                delta_usd: usd("-9500"),
            },
        )],
    );
    let refused = outcomes.last().expect("the withdrawal");
    assert_eq!(
        refused.rejection,
        Some(Rejection::EquityBelowExposure),
        "500 left against a 10 000 position"
    );
    assert_eq!(refused.snapshot.agent_equity, usd("10000"));
}

/// §5.2: for equities only regular-session sane marks move E, so an extended-hours print changes
/// nothing at all.
#[test]
#[ignore = "pending E6-4"]
fn extended_hours_marks_are_ignored_for_equities() {
    let document = swing(&[]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[marked(
            "2026-09-21T20:30:00.000000000Z",
            "105",
            MarketSession::AfterHours,
            true,
        )],
    );
    let ignored = outcomes.last().expect("the after-hours print");
    assert_eq!(
        ignored.snapshot.agent_equity,
        usd("10000"),
        "E still reflects the last regular-session mark"
    );
    assert_eq!(ignored.snapshot.high_water_mark, usd("10000"));
    assert!(ignored.journal.is_empty(), "nothing to journal");
}

/// §5.2: staleness is per instrument and measured in regular-session time, and a sane mark clears it
/// with its own event (§5.10: one event per restriction that changed).
#[test]
#[ignore = "pending E6-4"]
fn a_missing_mark_sets_stale_mark_and_a_sane_one_clears_it() {
    let document = swing(&[]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "100"),
            at_regular("2026-09-21T14:05:00.000000000Z", Input::Clock),
            mark("2026-09-21T14:06:00.000000000Z", "100"),
        ],
    );
    let stale = outcomes.get(1).expect("the tick");
    assert!(
        stale
            .snapshot
            .instrument_restrictions
            .contains(&InstrumentRestriction::StaleMark),
        "240 regular-session seconds without a mark is past the 120 s limit"
    );
    assert_eq!(
        stale.snapshot.agent_mode,
        AgentMode::Normal,
        "an instrument restriction never restricts the agent"
    );
    let cleared = outcomes.last().expect("the fresh mark");
    assert!(
        cleared.snapshot.instrument_restrictions.is_empty(),
        "a sane mark clears it"
    );
    assert!(
        cleared.journal.iter().any(|event| matches!(
            event,
            RiskEvent::InstrumentRestrictionChanged {
                restriction: InstrumentRestriction::StaleMark,
                active: false,
                ..
            }
        )),
        "the clear is its own event, got: {:?}",
        cleared.journal
    );
}

/// §5.9: an instrument removed from the working universe is restricted in that instrument only; the
/// agent stays `normal`.
#[test]
#[ignore = "pending E6-4"]
fn a_removed_instrument_is_exits_only_in_that_instrument() {
    let document = swing(&[]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[at_regular(
            "2026-09-21T14:01:00.000000000Z",
            Input::UniverseChanged {
                instrument: AssetId::parse(OTHER_INSTRUMENT).expect("a uuid"),
                change: UniverseChange::Removed,
                reason: RemovalReason::ThesisExpired,
            },
        )],
    );
    let removed = outcomes.last().expect("the removal");
    assert!(
        removed
            .snapshot
            .instrument_restrictions
            .contains(&InstrumentRestriction::RemovedInstrument)
    );
    assert!(
        removed.snapshot.restrictions.is_empty(),
        "no agent restriction, got: {:?}",
        removed.snapshot.restrictions
    );
    assert_eq!(removed.snapshot.agent_mode, AgentMode::Normal);
}

/// §3.1: a `profit_stop` is confirmed inside the risk state by breach time, not decided by the goal
/// rule, and completing it carries the `{reason, then}` shape of `GoalCompleted`.
#[test]
#[ignore = "pending E6-4"]
fn profit_stop_confirms_by_time_in_breach() {
    let document = swing(&[
        (
            "/goal",
            Some(goal_value(&[
                ("type", "profit_stop"),
                ("profit_level", "0.05"),
            ])),
        ),
        ("/risk/max_drawdown", Some(s("0.5"))),
        (
            "/risk/drawdown_ladder",
            Some(Value::Array(vec![rung_value(
                "0.4",
                "scale_sizes",
                Some("0.5"),
            )])),
        ),
    ]);
    let mut state = open(&document);
    let outcomes = fold(
        &mut state,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "106"),
            mark("2026-09-21T14:01:30.000000000Z", "106"),
            mark("2026-09-21T14:02:10.000000000Z", "106"),
        ],
    );
    let first = outcomes.first().expect("the first breaching mark");
    assert!(
        first.pending.contains(&LimitKey::ProfitStop),
        "E - C = 600 >= 0.05 x 10000, so it starts confirming, got pending {:?}",
        first.pending
    );
    let done = outcomes.last().expect("70 s in breach");
    assert!(
        done.journal.iter().any(|event| matches!(
            event,
            RiskEvent::GoalCompleted {
                reason: Some(GoalReason::ProfitStopReached),
                then: Some(ThenAction::DiscretionaryExitAllThenRetire),
                on_complete: None,
            }
        )),
        "a confirmed profit stop journals the `{{reason, then}}` shape, got: {:?}",
        done.journal
    );
}

/// §5.5: the size factor is the product of the active rungs' factors, and only the keys of the map
/// matter — a rung active for zero seconds is active.
#[test]
#[ignore = "pending E6-4"]
fn the_size_factor_is_the_product_of_active_rungs() {
    let ladder = [
        ladder_rung("0.03", LadderAction::ScaleSizes, Some("0.5")),
        ladder_rung("0.05", LadderAction::ScaleSizes, Some("0.6")),
        ladder_rung("0.08", LadderAction::FlattenAndPause, None),
    ];
    let one = Ratio::parse("1").expect("a ratio");
    assert_eq!(
        size_factor(&ladder, &[].into_iter().collect()).expect("evaluable"),
        one,
        "no active rung scales nothing"
    );
    assert_eq!(
        size_factor(&ladder, &[(0, 0)].into_iter().collect()).expect("evaluable"),
        Ratio::parse("0.5").expect("a ratio"),
        "a rung active for zero seconds is still active"
    );
    assert_eq!(
        size_factor(&ladder, &[(0, 900), (1, 60)].into_iter().collect()).expect("evaluable"),
        Ratio::parse("0.3").expect("a ratio"),
        "0.5 x 0.6, exactly"
    );
}

/// §5.8: after a reset the scale rungs lift highest `at` first, one at a time. Only `scale_sizes`
/// rungs are in the order at all — the other two actions lift by acknowledgment, not by a timer.
#[test]
#[ignore = "pending E6-4"]
fn rungs_lift_one_at_a_time_highest_first() {
    let ladder = [
        ladder_rung("0.03", LadderAction::ScaleSizes, Some("0.5")),
        ladder_rung("0.05", LadderAction::ScaleSizes, Some("0.6")),
        ladder_rung("0.06", LadderAction::ExitsOnly, None),
        ladder_rung("0.08", LadderAction::FlattenAndPause, None),
    ];
    assert_eq!(
        reset_lift_order(&ladder).expect("evaluable"),
        vec![1, 0],
        "the 0.05 rung lifts before the 0.03 one, and only scale rungs are listed"
    );
    assert_eq!(
        reset_lift_order(&ladder[2..]).expect("evaluable"),
        Vec::<u8>::new(),
        "a ladder with no scale rung lifts nothing on a timer"
    );
}

/// A mandate whose only limit in reach is the lifetime floor: the ladder is pushed out of the way so
/// the floor is the thing the test is about.
fn floor_document() -> Value {
    swing(&[
        ("/risk/breach_confirm_s", Some(i(0))),
        ("/risk/max_daily_loss", Some(s("0.5"))),
        ("/risk/max_drawdown", Some(s("0.5"))),
        (
            "/risk/drawdown_ladder",
            Some(Value::Array(vec![rung_value(
                "0.4",
                "scale_sizes",
                Some("0.5"),
            )])),
        ),
    ])
}

fn rung_value(at: &str, action: &str, factor: Option<&str>) -> Value {
    common::obj(vec![
        ("at", s(at)),
        ("action", s(action)),
        ("factor", factor.map_or(Value::Null, s)),
    ])
}

fn goal_value(members: &[(&str, &str)]) -> Value {
    let mut all: Vec<(&str, Value)> = members.iter().map(|(k, v)| (*k, s(v))).collect();
    all.push(("end_date", Value::Null));
    common::obj(all)
}

/// The base itself must parse before any of the above means anything: a scenario built on a document
/// the parser rejects would fail for the wrong reason.
#[test]
#[ignore = "pending E6-4"]
fn the_risk_scenarios_start_from_a_document_that_parses() {
    for document in [swing(&[]), floor_document(), base()] {
        let mandate = mandate_spec::Mandate::parse(&document)
            .unwrap_or_else(|e| panic!("a scenario document must parse: {e}"));
        assert_eq!(
            mandate.mandate_schema_version, 1,
            "the scenarios are v1 documents"
        );
        let _ = validated(&document);
    }
}
