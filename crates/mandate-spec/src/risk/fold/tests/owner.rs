//! Slice R4's inputs at their edges, and the two properties the slice owes: an allocation change
//! never triggers or lifts a limit (MI-2), and the ladder is monotone, so a deeper rung is never
//! engaged while a shallower one of its kind is not and sizes return from the highest rung down
//! (§5.5, §5.8). Each property's oracle shares no code with the fold: the first replays the same
//! walk with a clock tick in place of the change and recomputes §5.1's scaling and conditions on
//! `i128` counts of 10⁻¹² dollars, and the second rebuilds the latched rungs from the journal.

use mandate_domain::Environment;
use mandate_time::Date;

use super::*;
use crate::context::{AgentId, ContextArgs, JournaledFact};
use crate::document::{ConnectionId, OnComplete};
use crate::risk::{ApplyResult, Rejection};
use crate::validate::{ValidationContext, Violation, validate};

fn acknowledge(offset_s: i64, restriction: Latch) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session: MarketSession::Regular,
        input: Input::OwnerAcknowledged { restriction },
    })
}

fn allocate(offset_s: i64, delta: &str) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session: MarketSession::Regular,
        input: Input::AllocationChange {
            delta_usd: usd(delta)?,
        },
    })
}

fn loosen(offset_s: i64, fraction: &str, independent_approval: bool) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session: MarketSession::Regular,
        input: Input::FloorLoosened {
            new_max_loss_from_allocation: SchemaDec::parse(fraction, DecGrammar::OpenFraction)
                .map_err(|e| e.to_string())?,
            confirmed_at: at(1)?,
            independent_approval,
        },
    })
}

fn goal_complete(offset_s: i64) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session: MarketSession::Regular,
        input: Input::GoalComplete,
    })
}

fn retire(offset_s: i64) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session: MarketSession::Regular,
        input: Input::AgentStopped {
            reason: StopReason::OwnerStop,
        },
    })
}

fn acknowledged(limit: LimitKey) -> RiskEvent {
    RiskEvent::RiskLimitLifted {
        limit,
        action: None,
        reason: Some(LiftReason::OwnerAcknowledged),
    }
}

fn refused(reason: Rejection) -> RiskEvent {
    RiskEvent::MandateVersionApplied {
        result: ApplyResult::Rejected { reason },
    }
}

fn keys(state: &RiskState<'_>) -> Vec<u8> {
    state.snapshot().active_rungs.keys().copied().collect()
}

/// Three `scale_sizes` rungs at 2%, 3%, and 4% (each 0.5), then `exits_only` at 6% and
/// `flatten_and_pause` at 8%, confirming at once, with the daily loss out of the way.
fn three_scale_rungs(breach_confirm_s: &str) -> Result<ValidatedMandate, String> {
    ladder_only(&[
        (
            "/risk/drawdown_ladder",
            "[{\"at\": \"0.02\", \"action\": \"scale_sizes\", \"factor\": \"0.5\"}, \
              {\"at\": \"0.03\", \"action\": \"scale_sizes\", \"factor\": \"0.5\"}, \
              {\"at\": \"0.04\", \"action\": \"scale_sizes\", \"factor\": \"0.5\"}, \
              {\"at\": \"0.06\", \"action\": \"exits_only\", \"factor\": null}, \
              {\"at\": \"0.08\", \"action\": \"flatten_and_pause\", \"factor\": null}]",
        ),
        ("/risk/breach_confirm_s", breach_confirm_s),
    ])
}

/// After a reset only the highest active scale rung counts towards its lift (§5.8). A rung that
/// triggers again while sizes step back is the highest active one again, so it lifts next and its
/// shallower siblings wait: the active rungs stay the shallowest ones throughout. `ref.py` holds the
/// re-triggered rung back until the others have lifted, which would leave the 4% rung active with
/// the 3% rung lifted (DEC-167 item 8).
#[test]
fn sizes_step_back_from_the_highest_active_rung_and_a_re_triggered_rung_lifts_first()
-> Result<(), String> {
    let mandate = three_scale_rungs("0")?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    let latched = one(&mut state, mark(1, "94"))?;
    assert_eq!(keys(&state), [0, 1, 2]);
    assert_eq!(latched.snapshot.agent_mode, AgentMode::ExitsOnly);
    let reset = one(&mut state, acknowledge(2, Latch::DrawdownLadder))?;
    assert_eq!(
        reset.rejection, None,
        "an exits_only rung needs no flat book"
    );
    assert_eq!(
        reset.journal,
        [
            RiskEvent::HighWaterMarkReset {
                from: usd("10000")?,
                to: usd("9400")?,
            },
            acknowledged(LimitKey::DrawdownRung(3)),
            moved(AgentMode::ExitsOnly, AgentMode::Normal),
        ]
    );
    assert_eq!(one(&mut state, tick(601))?.journal, []);
    assert_eq!(one(&mut state, tick(602))?.journal, [scale_lifted(2)]);
    assert_eq!(keys(&state), [0, 1]);
    let again = one(&mut state, mark(603, "90.24"))?;
    assert_eq!(
        again.journal,
        [triggered(
            LimitKey::DrawdownRung(2),
            LadderAction::ScaleSizes,
            None
        )],
        "9400 − 9024 = 376 is 4% of 9400 exactly"
    );
    one(&mut state, mark(604, "94"))?;
    assert_eq!(
        one(&mut state, tick(1204))?.journal,
        [scale_lifted(2)],
        "the re-triggered rung is the highest active, so it lifts before the 3% rung"
    );
    assert_eq!(keys(&state), [0, 1]);
    assert_eq!(one(&mut state, tick(1803))?.journal, []);
    assert_eq!(one(&mut state, tick(1804))?.journal, [scale_lifted(1)]);
    assert_eq!(one(&mut state, tick(2404))?.journal, [scale_lifted(0)]);
    assert!(keys(&state).is_empty());
    Ok(())
}

/// An acknowledgment sets a lifted scale rung active again (`after_reset`), and ends a rung's hard
/// wait, journalling the cleared hard breach, so nothing is left pending (§5.6, §5.8).
#[test]
fn an_acknowledgment_re_arms_a_lifted_scale_rung_and_ends_a_rungs_hard_wait() -> Result<(), String>
{
    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    walk(
        &mut state,
        &[mark(1, "94")?, tick(61)?, mark(62, "99.5")?, tick(662)?],
    )?;
    assert!(keys(&state).is_empty(), "rung 0 lifted below 99");
    let reset = one(&mut state, acknowledge(663, Latch::DrawdownLadder))?;
    assert_eq!(
        reset.journal,
        [
            RiskEvent::HighWaterMarkReset {
                from: usd("10000")?,
                to: usd("9950")?,
            },
            acknowledged(LimitKey::DrawdownRung(1)),
            triggered(
                LimitKey::DrawdownRung(0),
                LadderAction::ScaleSizes,
                Some(TriggerReason::AfterReset)
            ),
            moved(AgentMode::ExitsOnly, AgentMode::Normal),
        ]
    );
    assert_eq!(reset.snapshot.size_factor, ratio("0.5")?);

    let mandate = ladder_only(&[
        ("/risk/drawdown_ladder/2/at", "\"0.07\""),
        ("/risk/max_drawdown", "\"0.07\""),
    ])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    let outcomes = walk(&mut state, &[mark(1, "94")?, tick(61)?, mark(62, "91.2")?])?;
    let waiting = outcomes.last().ok_or("three outcomes")?;
    assert_eq!(
        waiting.pending,
        BTreeSet::from([LimitKey::DrawdownRung(2)]),
        "8.8% is past the 7% rung's 8.75% hard level"
    );
    let reset = one(&mut state, acknowledge(63, Latch::DrawdownLadder))?;
    assert_eq!(
        reset.journal,
        [
            RiskEvent::HighWaterMarkReset {
                from: usd("10000")?,
                to: usd("9120")?,
            },
            acknowledged(LimitKey::DrawdownRung(1)),
            RiskEvent::RiskLimitLifted {
                limit: LimitKey::DrawdownRung(2),
                action: None,
                reason: Some(LiftReason::HardBreachCleared),
            },
            moved(AgentMode::ExitsOnly, AgentMode::Normal),
        ]
    );
    assert!(reset.pending.is_empty());
    assert!(reset.snapshot.restrictions.is_empty());
    Ok(())
}

/// An acknowledgment of something that is not latched, of a daily loss whose action is
/// `exits_only`, or of one already acknowledged is `nothing_to_acknowledge`; the floor is never
/// acknowledged; and a refused acknowledgment journals nothing and changes nothing (§5.8).
#[test]
fn an_acknowledgment_with_nothing_to_lift_is_refused_and_changes_nothing() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    for (latch, code) in [
        (Latch::DrawdownLadder, Rejection::NothingToAcknowledge),
        (Latch::DailyLoss, Rejection::NothingToAcknowledge),
        (Latch::LifetimeFloor, Rejection::NotAcknowledgeable),
    ] {
        let before = state.snapshot().clone();
        let outcome = one(&mut state, acknowledge(1, latch))?;
        assert_eq!(outcome.rejection, Some(code), "{latch:?}");
        assert!(outcome.journal.is_empty(), "{latch:?}");
        assert_eq!(outcome.snapshot, before, "{latch:?}");
    }

    let mut state = open(&daily("exits_only")?, &equity()?, &Every)?;
    walk(&mut state, &[mark(1, "98.75")?, mark(11, "98.75")?])?;
    assert!(state.snapshot().latched.contains(&LimitKey::MaxDailyLoss));
    assert_eq!(
        one(&mut state, acknowledge(12, Latch::DailyLoss))?.rejection,
        Some(Rejection::NothingToAcknowledge),
        "an exits_only daily loss lifts by itself"
    );

    let mut state = open(&daily("flatten_and_pause")?, &equity()?, &Every)?;
    walk(
        &mut state,
        &[
            mark(1, "98.75")?,
            mark(11, "98.75")?,
            fill(12, Side::Sell, "100", "98.75")?,
        ],
    )?;
    let first = one(&mut state, acknowledge(13, Latch::DailyLoss))?;
    assert_eq!(first.rejection, None);
    assert_eq!(
        first.journal,
        [moved(AgentMode::Paused, AgentMode::ExitsOnly)]
    );
    let second = one(&mut state, acknowledge(14, Latch::DailyLoss))?;
    assert_eq!(second.rejection, Some(Rejection::NothingToAcknowledge));
    assert!(second.journal.is_empty());
    Ok(())
}

/// A renewal writes the daily latch afresh (DEC-167 item 7 (c)), so an acknowledged
/// `flatten_and_pause` daily loss that renews pauses the agent again and waits for a new
/// acknowledgment before it lifts (DEC-167 item 8). Once a new day and `daily_breach_min_s` have
/// passed, the lift comes with the acknowledgment itself (§5.4).
#[test]
fn a_renewed_daily_flatten_pauses_again_and_waits_for_a_new_acknowledgment() -> Result<(), String> {
    let key = LimitKey::MaxDailyLoss;
    let mut state = open(&daily("flatten_and_pause")?, &equity()?, &Every)?;
    let outcomes = walk(
        &mut state,
        &[
            mark(1, "98.75")?,
            mark(11, "98.75")?,
            fill(12, Side::Sell, "100", "98.75")?,
            acknowledge(13, Latch::DailyLoss)?,
            new_day(14)?,
            fill(15, Side::Buy, "100", "98.75")?,
            mark(16, "97.5")?,
            new_day(86_400)?,
        ],
    )?;
    assert_eq!(
        outcomes.get(6).map(|o| o.journal.clone()),
        Some(vec![
            triggered(LimitKey::DrawdownRung(0), LadderAction::ScaleSizes, None),
            triggered(
                key,
                LadderAction::FlattenAndPause,
                Some(TriggerReason::NewDayBreach)
            ),
            moved(AgentMode::ExitsOnly, AgentMode::Paused),
        ]),
        "−125 against a day start of 9875 is past its 1.25x level of 123.4375"
    );
    assert_eq!(
        outcomes.get(7).map(|o| o.journal.clone()),
        Some(vec![day_started("9750")?]),
        "a day and the minimum have passed, but not a new acknowledgment"
    );
    let lifted = walk(
        &mut state,
        &[
            fill(86_401, Side::Sell, "100", "97.5")?,
            acknowledge(86_402, Latch::DailyLoss)?,
        ],
    )?;
    assert_eq!(
        lifted.last().map(|o| o.journal.clone()),
        Some(vec![
            RiskEvent::RiskLimitLifted {
                limit: key,
                action: None,
                reason: None,
            },
            moved(AgentMode::Paused, AgentMode::Normal),
        ]),
        "the new day and the minimum have passed, so the new acknowledgment is all the lift waits for"
    );
    Ok(())
}

/// An allocation change settles time at its instant first, so a breach that confirms there latches
/// before the change is judged, and the increase is refused (§5.1, MI-7). At an equity that is not
/// positive §5.1's ratio cannot be formed, so the step is refused and changes nothing. With a flat
/// book a change must leave equity positive.
#[test]
fn an_allocation_change_settles_time_first_and_needs_a_positive_equity() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    one(&mut state, mark(1, "94"))?;
    let settled = one(&mut state, allocate(61, "100"))?;
    assert_eq!(
        settled.journal,
        [
            triggered(LimitKey::DrawdownRung(1), LadderAction::ExitsOnly, None),
            moved(AgentMode::Normal, AgentMode::ExitsOnly),
            refused(Rejection::IncreaseBlockedWhileLatched),
        ]
    );
    assert_eq!(
        settled.rejection,
        Some(Rejection::IncreaseBlockedWhileLatched)
    );

    let leveraged = opening(AssetClass::UsEquity, "200", "0", 1_000_000)?;
    let mut state = open(&mandate, &leveraged, &Every)?;
    one(&mut state, mark(1, "50"))?;
    assert_eq!(state.snapshot().agent_equity, usd("0")?);
    let before = state.clone();
    assert_eq!(
        state
            .step(&allocate(2, "1")?)
            .map(|_| ())
            .map_err(|e| e.code()),
        Err("invalid_input")
    );
    assert_eq!(state, before, "the refused step changed nothing");

    let mut state = open(&mandate, &equity()?, &Every)?;
    one(&mut state, fill(1, Side::Sell, "100", "100"))?;
    assert_eq!(
        one(&mut state, allocate(2, "-10000"))?.rejection,
        Some(Rejection::EquityBelowExposure),
        "a flat book still needs a positive equity"
    );
    let applied = one(&mut state, allocate(3, "-9999"))?;
    assert_eq!(applied.rejection, None);
    assert_eq!(applied.snapshot.agent_equity, usd("1")?);
    assert_eq!(applied.snapshot.high_water_mark, usd("1")?);
    assert_eq!(applied.snapshot.net_contributed, usd("1")?);
    Ok(())
}

/// A loosening version lifts a latched floor for a single user only once the first full risk day
/// after the confirmation day has ended, here 00:00 New York on 2026-09-23 (§5.7). A loosening of
/// an unlatched floor applies at once and lifts nothing, and a version that does not raise the
/// fraction is `not_loosening`.
#[test]
fn a_single_user_loosens_a_latched_floor_at_the_end_of_the_first_full_day() -> Result<(), String> {
    let mandate = ladder_only(&[("/risk/breach_confirm_s", "0")])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    one(&mut state, mark(1, "90"))?;
    assert!(state.snapshot().latched.contains(&LimitKey::LifetimeFloor));
    let full_day_ends = 38 * 3_600;
    assert_eq!(
        one(&mut state, loosen(full_day_ends - 1, "0.2", false))?.rejection,
        Some(Rejection::WaitingPeriod)
    );
    let lifted = one(&mut state, loosen(full_day_ends, "0.2", false))?;
    assert_eq!(lifted.rejection, None);
    assert!(!lifted.snapshot.latched.contains(&LimitKey::LifetimeFloor));

    let mut state = open(&mandate, &equity()?, &Every)?;
    let applied = one(&mut state, loosen(1, "0.2", false))?;
    assert_eq!(
        applied.journal,
        [RiskEvent::MandateVersionApplied {
            result: ApplyResult::Applied {
                allocation_change: None,
                max_loss_from_allocation: Some(
                    SchemaDec::parse("0.2", DecGrammar::OpenFraction).map_err(|e| e.to_string())?
                ),
            },
        }]
    );
    for fraction in ["0.2", "0.15"] {
        assert_eq!(
            one(&mut state, loosen(2, fraction, true))?.journal,
            [refused(Rejection::NotLoosening)],
            "{fraction} does not raise 0.2"
        );
    }
    let marked = one(&mut state, mark(3, "89"))?;
    assert!(
        !marked.pending.contains(&LimitKey::LifetimeFloor),
        "8900 is below the old 9000 floor and above the new 8000"
    );
    Ok(())
}

/// A `profit_stop` goal has no `on_complete`: completing it applies its one outcome under
/// `goal_complete` (§3.1). A retired agent's profit stop is no longer confirmed, and retiring at a
/// gain carries no loss to the connection (§5.7).
#[test]
fn a_profit_stop_completes_to_its_one_outcome_and_retirement_ends_its_confirmation()
-> Result<(), String> {
    let mandate = patched(&[
        (
            "/goal",
            "{\"type\": \"profit_stop\", \"profit_level\": \"0.1\", \"end_date\": null}",
        ),
        ("/risk/breach_confirm_s", "0"),
        ("/risk/max_daily_loss", "\"0.5\""),
    ])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    let done = one(&mut state, goal_complete(1))?;
    assert_eq!(
        done.journal,
        [
            RiskEvent::GoalCompleted {
                reason: None,
                then: Some(ThenAction::DiscretionaryExitAllThenRetire),
                on_complete: None,
            },
            moved(AgentMode::Normal, AgentMode::ExitsOnly),
        ]
    );
    assert_eq!(
        done.snapshot.restrictions,
        BTreeSet::from([Restriction::GoalComplete])
    );

    let mut state = open(&mandate, &equity()?, &Every)?;
    one(&mut state, mark(1, "105"))?;
    let stopped = one(&mut state, retire(2))?;
    assert_eq!(
        stopped.journal,
        [
            RiskEvent::AgentStopped {
                reason: StopReason::OwnerStop,
                loss_carry_usd: usd("0")?,
            },
            moved(AgentMode::Normal, AgentMode::Stopped),
        ],
        "a gain of 500 carries nothing"
    );
    let above = one(&mut state, mark(3, "111"))?;
    assert!(
        above.journal.is_empty() && above.pending.is_empty(),
        "{:?}",
        above.journal
    );
    Ok(())
}

/// `release` retires the agent as any retirement does (DEC-270): `AgentStopped` follows
/// `PositionReleased` with the net dollar loss, valued at the last mark, so
/// `ValidationContext::from_journal` carries it to the connection. A redeploy there opens at that L,
/// its floor 150 above C × (1 − f), and one whose floor budget the carry uses is refused by V-032, so
/// releasing and redeploying cannot reset the floor (§5.7, MI-14).
#[test]
fn a_release_retires_with_its_loss_carry_and_a_redeploy_opens_at_it() -> Result<(), String> {
    let released = ladder_only(&[
        ("/goal/on_complete", "\"release\""),
        ("/risk/breach_confirm_s", "0"),
    ])?;
    let mut state = open(&released, &equity()?, &Every)?;
    one(&mut state, mark(1, "98.5"))?;
    let done = one(&mut state, goal_complete(2))?;
    assert_eq!(
        done.journal,
        [
            RiskEvent::GoalCompleted {
                reason: None,
                then: None,
                on_complete: Some(OnComplete::Release),
            },
            RiskEvent::PositionReleased { qty: qty("100")? },
            RiskEvent::AgentStopped {
                reason: StopReason::GoalComplete,
                loss_carry_usd: usd("150")?,
            },
            moved(AgentMode::Normal, AgentMode::Stopped),
        ],
        "E = 9,850 against N = 10,000"
    );
    let carried = done
        .journal
        .iter()
        .find_map(|event| match event {
            RiskEvent::AgentStopped { loss_carry_usd, .. } => Some(*loss_carry_usd),
            _ => None,
        })
        .ok_or("the release journals AgentStopped")?;

    let connection = ConnectionId::parse("conn_alpaca_paper_01").map_err(|e| e.to_string())?;
    let date = Date::parse("2026-09-23").map_err(|e| e.to_string())?;
    let facts = [
        JournaledFact::AgentVersionActive {
            agent: AgentId::new("released"),
            connection_id: connection.clone(),
            environment: Environment::Paper,
            allocation_usd: usd("10000")?,
            pinned: BTreeSet::new(),
        },
        JournaledFact::AgentStopped {
            agent: AgentId::new("released"),
            connection_id: connection.clone(),
            retired_on: date,
            loss_added_usd: carried,
        },
    ];
    let args = ContextArgs {
        agent: AgentId::new("redeployed"),
        connection_id: connection,
        validation_date: date,
        membership: None,
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    };
    let read =
        ValidationContext::from_journal(&mandate(&[])?, args, &facts).map_err(|e| e.to_string())?;
    assert_eq!(read.connection_loss_carry_usd, usd("150")?);

    let redeployed = ladder_only(&[("/risk/breach_confirm_s", "0")])?;
    for (inherited, floors) in [(read.connection_loss_carry_usd, true), (Usd::ZERO, false)] {
        let opening = Opening {
            inherited_loss_usd: inherited,
            ..equity()?
        };
        let mut state = open(&redeployed, &opening, &Every)?;
        assert_eq!(state.snapshot().inherited_loss, inherited);
        let at_floor = one(&mut state, mark(1, "91.5"))?;
        assert_eq!(
            at_floor
                .snapshot
                .restrictions
                .contains(&Restriction::LifetimeFloor),
            floors,
            "E = 9,150 is the floor 9,000 + L only with the carry: {:?}",
            at_floor.journal
        );
    }

    let mut with_carry = context()?;
    with_carry.connection_loss_carry_usd = read.connection_loss_carry_usd;
    let small = mandate(&[
        ("/capital/allocation_usd", "\"1500\""),
        ("/risk/max_gross_exposure_usd", "\"1500\""),
    ])?;
    assert_eq!(
        validate(&small, &with_carry)
            .map_err(|e| e.to_string())?
            .violations,
        BTreeSet::from([Violation::V032]),
        "a 1,500 allocation's floor budget is 150, which the carry uses"
    );
    Ok(())
}

/// `disarm_ladder` drops what the ladder and the daily loss were confirming, a breach carried over
/// the rollover included, so a later rollover has nothing to refuse, while the floor keeps
/// confirming and latches (§3.1, §5.7).
#[test]
fn a_disarmed_ladder_drops_its_confirmations_and_the_floor_keeps_its_own() -> Result<(), String> {
    let mandate = patched(&[("/goal/on_complete", "\"disarm_ladder\"")])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    walk(&mut state, &[mark(1, "89")?, new_day(2)?])?;
    let disarmed = one(&mut state, goal_complete(3))?;
    assert_eq!(
        disarmed.journal.first(),
        Some(&RiskEvent::GoalCompleted {
            reason: None,
            then: None,
            on_complete: Some(OnComplete::DisarmLadder),
        })
    );
    assert_eq!(
        disarmed.pending,
        BTreeSet::from([LimitKey::LifetimeFloor]),
        "only the floor is still confirming"
    );
    assert_eq!(one(&mut state, new_day(4))?.journal, [day_started("8900")?]);
    let floored = one(&mut state, tick(62))?;
    assert_eq!(
        floored.journal,
        [
            triggered(LimitKey::LifetimeFloor, LadderAction::FlattenAndPause, None),
            RiskEvent::KillSwitchActivated {
                scope: KillScope::Agent,
                initiator: LimitKey::LifetimeFloor,
            },
            moved(AgentMode::ExitsOnly, AgentMode::Paused),
        ]
    );
    Ok(())
}

/// The oracle's arithmetic: a step that does not fit `i128` fails the test rather than wrapping.
fn fits(value: Option<i128>) -> Result<i128, String> {
    value.ok_or_else(|| "the oracle's arithmetic overflowed i128".to_owned())
}

fn mul(left: i128, right: i128) -> Result<i128, String> {
    fits(left.checked_mul(right))
}

fn add(left: i128, right: i128) -> Result<i128, String> {
    fits(left.checked_add(right))
}

fn sub(left: i128, right: i128) -> Result<i128, String> {
    fits(left.checked_sub(right))
}

/// A figure in 10⁻¹² dollars. Every figure these walks reach has at most 12 places.
fn units(amount: Usd) -> Result<i128, String> {
    let text = amount.to_string();
    let (negative, digits) = text
        .strip_prefix('-')
        .map_or((false, text.as_str()), |rest| (true, rest));
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if fraction.len() > 12 {
        return Err(format!("{text} has more than 12 places"));
    }
    let magnitude: i128 = format!("{whole}{fraction:0<12}")
        .parse()
        .map_err(|_| text.clone())?;
    if negative {
        sub(0, magnitude)
    } else {
        Ok(magnitude)
    }
}

/// ceil(x × numerator ÷ denominator) for a positive denominator and non-negative operands.
fn ceil_scaled(x: i128, numerator: i128, denominator: i128) -> Result<i128, String> {
    let rounded_up = sub(add(mul(x, numerator)?, denominator)?, 1)?;
    fits(rounded_up.checked_div_euclid(denominator))
}

/// The base mandate's conditions on 10⁻¹² counts, each `(soft, hard)`: rungs at 2%, 5%, and 8%, the
/// 2% daily loss, and the 10% floor, with 1.25x hard levels (§5.2, §5.6).
fn oracle_conditions(
    e: i128,
    h: i128,
    e0: i128,
    c: i128,
    l: i128,
) -> Result<Vec<(bool, bool)>, String> {
    let fall = sub(h, e)?;
    let mut out = Vec::new();
    for at in [20_i128, 50, 80] {
        out.push((
            mul(1_000, fall)? >= mul(at, h)?,
            mul(100_000, fall)? >= mul(mul(125, at)?, h)?,
        ));
    }
    let day = sub(e, e0)?;
    out.push((
        mul(100, day)? <= mul(-2, e0)?,
        mul(10_000, day)? <= mul(-250, e0)?,
    ));
    out.push((
        mul(10, e)? <= add(mul(9, c)?, mul(10, l)?)?,
        mul(1_000, e)? <= add(mul(875, c)?, mul(1_000, l)?)?,
    ));
    Ok(out)
}

/// A positive count of tenths as canonical decimal text.
fn tenths_text(tenths: i64) -> String {
    let digits = format!("{tenths:02}");
    let (whole, tenth) = digits.split_at(digits.len().saturating_sub(1));
    if tenth == "0" {
        whole.to_owned()
    } else {
        format!("{whole}.{tenth}")
    }
}

/// One input before the allocation change: a sane regular mark or a tick, after a gap.
#[derive(Debug, Clone, Copy)]
enum Before {
    Mark(i64),
    Tick,
}

proptest! {
    /// An allocation change never triggers or lifts a limit (§5.1, MI-2). The oracle runs the same
    /// walk twice, once with the change and once with a clock tick at its instant, which is what
    /// settling time is. Refused, the change leaves exactly the tick's state and journals only its
    /// refusal; applied, it leaves the tick's latches, restrictions, rungs, mode, and pending limits
    /// and journals only itself, and its figures are §5.1's, recomputed on `i128` counts. The reason
    /// it is refused for is recomputed the same way.
    #[test]
    fn an_allocation_change_never_triggers_or_lifts_a_limit(
        walk_before in prop::collection::vec(
            (1_i64..=90, prop_oneof![
                (180_i64..=212).prop_map(Before::Mark),
                Just(Before::Tick),
            ]),
            1..8,
        ),
        delta in -6_000_i64..6_000,
        inherited in prop::sample::select(vec!["0", "300"]),
        confirm in prop::sample::select(vec!["0", "60"]),
    ) {
        prop_assume!(delta != 0);
        let mandate = patched(&[("/risk/breach_confirm_s", confirm)])
            .map_err(TestCaseError::fail)?;
        let book = opening(AssetClass::UsEquity, "50", inherited, 1_000_000)
            .map_err(TestCaseError::fail)?;
        let mut now = 0_i64;
        let mut bid_halves = 200_i64;
        let mut steps = Vec::new();
        for (gap, input) in &walk_before {
            now = now.saturating_add(*gap);
            steps.push(match input {
                Before::Mark(halves) => {
                    bid_halves = *halves;
                    mark(now, &tenths_text(halves.saturating_mul(5)))
                }
                Before::Tick => tick(now),
            }.map_err(TestCaseError::fail)?);
        }
        let change_at = now.saturating_add(30);
        let mut with_tick = steps.clone();
        with_tick.push(tick(change_at).map_err(TestCaseError::fail)?);
        let mut with_change = steps;
        with_change.push(allocate(change_at, &delta.to_string()).map_err(TestCaseError::fail)?);

        let mut ticked = open(&mandate, &book, &Every).map_err(TestCaseError::fail)?;
        let tick_out = walk(&mut ticked, &with_tick).map_err(TestCaseError::fail)?;
        let mut changed = open(&mandate, &book, &Every).map_err(TestCaseError::fail)?;
        let change_out = walk(&mut changed, &with_change).map_err(TestCaseError::fail)?;
        let settled = tick_out.last().ok_or_else(|| TestCaseError::fail("a tick"))?;
        let outcome = change_out.last().ok_or_else(|| TestCaseError::fail("a change"))?;
        let s = &settled.snapshot;
        let u = |amount: Usd| units(amount).map_err(TestCaseError::fail);
        let (e, h, e0, c, l) = (
            u(s.agent_equity)?,
            u(s.high_water_mark)?,
            u(s.day_start_equity)?,
            u(s.capital_base)?,
            u(s.inherited_loss)?,
        );
        let oracle = |result: Result<i128, String>| result.map_err(TestCaseError::fail);
        let d = oracle(mul(i128::from(delta), 1_000_000_000_000))?;
        let e1 = oracle(add(e, d))?;
        let exposure = oracle(mul(i128::from(bid_halves), 25_000_000_000_000))?;
        let (h1, e01, c1, l1) = (
            oracle(ceil_scaled(h, e1, e))?,
            oracle(ceil_scaled(e0, e1, e))?,
            oracle(ceil_scaled(c, e1, e))?,
            oracle(ceil_scaled(l, e1, e))?,
        );
        let newly_true = oracle_conditions(e, h, e0, c, l)
            .map_err(TestCaseError::fail)?
            .into_iter()
            .zip(oracle_conditions(e1, h1, e01, c1, l1).map_err(TestCaseError::fail)?)
            .any(|((soft, hard), (soft1, hard1))| (soft1 && !soft) || (hard1 && !hard));
        let expected = if delta > 0 && !s.latched.is_empty() {
            Some(Rejection::IncreaseBlockedWhileLatched)
        } else if e1 <= 0 || e1 < exposure {
            Some(Rejection::EquityBelowExposure)
        } else if newly_true {
            Some(Rejection::WouldTriggerLimit)
        } else {
            None
        };
        prop_assert_eq!(outcome.rejection, expected, "E {} delta {}", s.agent_equity, delta);
        let mut journal = settled.journal.clone();
        journal.push(match expected {
            Some(reason) => refused(reason),
            None => RiskEvent::MandateVersionApplied {
                result: ApplyResult::Applied {
                    allocation_change: Some(usd(&delta.to_string()).map_err(TestCaseError::fail)?),
                    max_loss_from_allocation: None,
                },
            },
        });
        prop_assert_eq!(&outcome.journal, &journal, "the change journals only itself");
        prop_assert_eq!(&outcome.pending, &settled.pending);
        let after = &outcome.snapshot;
        if expected.is_some() {
            prop_assert_eq!(after, s, "a refused change changes nothing");
            return Ok(());
        }
        prop_assert_eq!(&after.latched, &s.latched, "MI-2: nothing triggers or lifts");
        prop_assert_eq!(&after.restrictions, &s.restrictions);
        prop_assert_eq!(&after.active_rungs, &s.active_rungs);
        prop_assert_eq!(after.agent_mode, s.agent_mode);
        prop_assert_eq!(after.size_factor, s.size_factor);
        prop_assert_eq!(
            (u(after.agent_equity)?, u(after.high_water_mark)?, u(after.day_start_equity)?),
            (e1, h1, e01)
        );
        prop_assert_eq!(
            (u(after.capital_base)?, u(after.inherited_loss)?, u(after.net_contributed)?),
            (c1, l1, oracle(add(u(s.net_contributed)?, d))?)
        );
        prop_assert!(after.drawdown >= s.drawdown, "MI-2: the drawdown never falls");
        prop_assert!(after.daily_pnl_fraction <= s.daily_pnl_fraction);
    }
}

/// One input to the ladder walk.
#[derive(Debug, Clone, Copy)]
enum Move {
    /// A sane mark, in tenths of a dollar.
    Mark(i64),
    Tick,
    Acknowledge,
}

proptest! {
    /// The ladder is monotone (§5.5, §5.8). Scale rungs trigger at a drawdown that hits every
    /// shallower one too and lift from the highest down, so at every step the active scale rungs are
    /// the shallowest ones and the size factor is 0.5 to their number; the latched rungs are the
    /// shallowest confirmable ones too, since a deeper breach is a shallower one. The latched rungs
    /// the journal accounts for, rebuilt from the triggers and lifts it carries, are the ones the
    /// state reports, and a latched rung lifts only at an applied acknowledgment of the ladder
    /// (MI-3).
    #[test]
    fn the_ladder_is_monotone(
        moves in prop::collection::vec(
            (1_i64..=300, prop_oneof![
                4 => (900_i64..=1_050).prop_map(Move::Mark),
                2 => Just(Move::Tick),
                1 => Just(Move::Acknowledge),
            ]),
            1..40,
        ),
        confirm in prop::sample::select(vec!["0", "30"]),
        lift_after in prop::sample::select(vec!["0", "60", "600"]),
    ) {
        let mandate = ladder_only(&[
            (
                "/risk/drawdown_ladder",
                "[{\"at\": \"0.02\", \"action\": \"scale_sizes\", \"factor\": \"0.5\"}, \
                  {\"at\": \"0.03\", \"action\": \"scale_sizes\", \"factor\": \"0.5\"}, \
                  {\"at\": \"0.04\", \"action\": \"scale_sizes\", \"factor\": \"0.5\"}, \
                  {\"at\": \"0.06\", \"action\": \"exits_only\", \"factor\": null}, \
                  {\"at\": \"0.08\", \"action\": \"flatten_and_pause\", \"factor\": null}]",
            ),
            ("/risk/breach_confirm_s", confirm),
            ("/risk/scale_lift_after_s", lift_after),
            ("/capital/max_loss_from_allocation", "\"0.2\""),
        ])
        .map_err(TestCaseError::fail)?;
        let mut state = open(&mandate, &equity().map_err(TestCaseError::fail)?, &Every)
            .map_err(TestCaseError::fail)?;
        let factors = ["1", "0.5", "0.25", "0.125"];
        let mut now = 0_i64;
        let mut journalled: BTreeSet<LimitKey> = BTreeSet::new();
        for (gap, step) in moves {
            now = now.saturating_add(gap);
            let input = match step {
                Move::Mark(tenths) => mark(now, &tenths_text(tenths)),
                Move::Tick => tick(now),
                Move::Acknowledge => acknowledge(now, Latch::DrawdownLadder),
            }
            .map_err(TestCaseError::fail)?;
            let outcome = one(&mut state, Ok(input)).map_err(TestCaseError::fail)?;
            let snapshot = &outcome.snapshot;
            let active: Vec<u8> = snapshot.active_rungs.keys().copied().collect();
            let shallowest: Vec<u8> = (0..3).take(active.len()).collect();
            prop_assert_eq!(&active, &shallowest, "at {} s after {:?}", now, step);
            prop_assert_eq!(
                snapshot.size_factor,
                ratio(factors.get(active.len()).copied().unwrap_or("0"))
                    .map_err(TestCaseError::fail)?
            );
            let latched: Vec<u8> = snapshot
                .latched
                .iter()
                .filter_map(|key| match key {
                    LimitKey::DrawdownRung(index) => Some(*index),
                    _ => None,
                })
                .collect();
            let confirmable: Vec<u8> = (3..5).take(latched.len()).collect();
            prop_assert_eq!(&latched, &confirmable, "at {} s after {:?}", now, step);
            let applied_acknowledgment =
                matches!(step, Move::Acknowledge) && outcome.rejection.is_none();
            for event in &outcome.journal {
                match event {
                    RiskEvent::RiskLimitTriggered { limit, action, reason }
                        if *action != LadderAction::ScaleSizes
                            && *reason != Some(TriggerReason::HardBreachPending) =>
                    {
                        journalled.insert(*limit);
                    }
                    RiskEvent::RiskLimitLifted { limit, reason: Some(LiftReason::OwnerAcknowledged), .. } => {
                        prop_assert!(applied_acknowledgment, "{:?} after {:?}", event, step);
                        journalled.remove(limit);
                    }
                    _ => {}
                }
            }
            let reported: BTreeSet<LimitKey> = snapshot
                .latched
                .iter()
                .filter(|key| matches!(key, LimitKey::DrawdownRung(_)))
                .copied()
                .collect();
            prop_assert_eq!(&journalled, &reported, "at {} s after {:?}", now, step);
        }
    }
}
