//! The risk-state fold (`kind: risk_state`, MC-R01 to MC-R24; [§5]).
//!
//! §5 is one transition function with ten kinds of input, and almost every rule in it is about *time*:
//! breach time that accumulates over intervals credited by the condition at their start, a hard breach
//! that waits for a second quote, a breach that keeps confirming against the day it began in, scale rungs
//! that lift one at a time, a floor that lifts only after a full risk day. The 24 reference cases pin
//! points on those timelines. What they cannot pin is the shape of the timeline between the points, which
//! is where an implementation goes wrong: a confirmation that resets on the first quiet interval, a hard
//! level that latches on one print, a rollover measured against the wrong day's E₀.
//!
//! Every figure below is recomputed from §5's own formulas rather than copied from a case, and the
//! properties carry three oracles that share no code with the crate: an interval accumulator for breach
//! time, an `i128` accumulator at 10⁻¹² for equity and the high-water mark, and a journal reader that
//! rebuilds the latches, the restrictions, and the mode from the events a step emitted.
//!
//! [§5]: ../../../docs/specs/mandate.md#5-risk-state-and-limits

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{ASSET_A, arr, obj, s, with, with_all};
use mandate_canon::Value;
use mandate_domain::{AgentMode, AssetClass, AssetId, Environment, MarketSession, Side};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_spec::document::ProvenanceMap;
use mandate_spec::risk::{
    ApplyResult, Confirmation, Input, InstrumentRestriction, Latch, LimitKey, Opening, Outcome,
    RemovalReason, Restriction, RiskEvent, RiskState, SessionClock, Snapshot, Step, StopReason,
    TriggerReason, UniverseChange, size_factor,
};
use mandate_spec::validate::{ValidatedMandate, ValidationContext};
use mandate_spec::{DecGrammar, Mandate, SchemaDec, SpecError};
use mandate_time::{Date, ExchangeCalendar, Session, UtcNanos};
use proptest::prelude::*;

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("an instant")
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("a dollar amount")
}

fn qty(text: &str) -> Qty {
    Qty::parse(text).expect("a quantity")
}

fn price(text: &str) -> Price {
    Price::parse(text).expect("a price")
}

fn ratio(text: &str) -> Ratio {
    Ratio::parse(text).expect("a ratio")
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
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
    }
}

/// `common::base()` with the changes a test needs: allocation 10000, `max_loss_from_allocation` 0.1, the
/// three-rung ladder (0.03 `scale_sizes` factor 0.5, 0.06 `exits_only`, 0.08 `flatten_and_pause`),
/// hysteresis 0.01, `breach_confirm_s` 60, `daily_breach_min_s` 3600, `scale_lift_after_s` 600,
/// `max_daily_loss` 0.02 acting `exits_only`, and a `continuous` goal, so a test changes only the one
/// thing it is about.
fn swing(changes: &[(&str, Option<Value>)]) -> Result<ValidatedMandate, String> {
    let document = Mandate::parse(&with_all(changes))
        .map_err(|e| format!("the risk-state document must parse: {e} ({})", e.code()))?;
    ValidatedMandate::new(document, &context(), &[])
        .map_err(|e| format!("the risk-state document must validate: {e}"))
}

/// The same base with the timers a test wants out of the way, stated once so no test hides a limit it did
/// not mean to disarm. `max_daily_loss` of 0.5 isolates the ladder, exactly as the R cases do.
fn ladder_only(changes: &[(&str, Option<Value>)]) -> Result<ValidatedMandate, String> {
    let mut all: Vec<(&str, Option<Value>)> = vec![
        ("/risk/max_daily_loss", Some(s("0.5"))),
        ("/risk/breach_confirm_s", Some(common::i(0))),
        ("/risk/scale_lift_after_s", Some(common::i(0))),
        ("/risk/daily_breach_min_s", Some(common::i(0))),
    ];
    all.extend_from_slice(changes);
    swing(&all)
}

/// `common::base()` with a single pointer changed, which is all a test about one figure needs.
fn one_change(path: &str, value: Value) -> Result<ValidatedMandate, String> {
    let document = Mandate::parse(&with(path, Some(value)))
        .map_err(|e| format!("the risk-state document must parse: {e} ({})", e.code()))?;
    ValidatedMandate::new(document, &context(), &[])
        .map_err(|e| format!("the risk-state document must validate: {e}"))
}

/// The same base as a crypto mandate: one asset class, both pinned instruments in it (V-039), and the
/// crypto stop-limit offset protection needs when the class is there (V-008).
fn accumulator(changes: &[(&str, Option<Value>)]) -> Result<ValidatedMandate, String> {
    let mut all: Vec<(&str, Option<Value>)> = vec![
        ("/universe/asset_classes", Some(arr(vec![s("crypto")]))),
        (
            "/universe/pinned_instruments/0/asset_class",
            Some(s("crypto")),
        ),
        (
            "/universe/pinned_instruments/1/asset_class",
            Some(s("crypto")),
        ),
        ("/protection/crypto_stop_limit_offset", Some(s("0.005"))),
    ];
    all.extend_from_slice(changes);
    swing(&all)
}

/// A ladder value for a patch: `at`, `action`, and the factor a `scale_sizes` rung carries.
fn rung(level: &str, action: &str, factor: Option<&str>) -> Value {
    obj(vec![
        ("at", s(level)),
        ("action", s(action)),
        ("factor", factor.map_or(Value::Null, s)),
    ])
}

/// 100 shares bought at 100 on an allocation of 10000, so E starts at 10000 and every mark moves it by
/// 100 × the bid, which is what makes the drawdown arithmetic below readable.
fn opening(when: &str, position: &str, avg_cost: &str, inherited_loss: &str) -> Opening {
    Opening {
        position_qty: qty(position),
        avg_cost: price(avg_cost),
        asset_class: AssetClass::UsEquity,
        at: at(when),
        inherited_loss_usd: usd(inherited_loss),
        mark_max_age_s: 120,
    }
}

/// The same opening with a staleness limit no test but the staleness one can reach, so a walk that is
/// about a limit does not also journal an instrument restriction it never meant to exercise.
fn patient_opening(when: &str, position: &str, avg_cost: &str, inherited_loss: &str) -> Opening {
    Opening {
        mark_max_age_s: 86_400,
        ..opening(when, position, avg_cost, inherited_loss)
    }
}

fn retire(when: &str) -> Step {
    Step {
        at: at(when),
        session: MarketSession::Regular,
        input: Input::AgentStopped {
            reason: StopReason::OwnerStop,
        },
    }
}

/// The clock §5.2 and §5.5 count an equity's staleness and lift delays on: regular-session seconds from
/// `mandate-time`'s NYSE calendar. This crate holds no calendar, which is why the clock is a parameter.
struct RegularSessionClock {
    calendar: ExchangeCalendar,
}

impl RegularSessionClock {
    fn new() -> Self {
        Self {
            calendar: ExchangeCalendar::us_equities().expect("the US equities calendar"),
        }
    }
}

impl SessionClock for RegularSessionClock {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        let mut total: u64 = 0;
        let mut date = from.date();
        while date <= to.date() {
            for span in self.calendar.sessions(date)? {
                if span.session() != Session::Regular {
                    continue;
                }
                let start = span.start().max(from);
                let end = span.end().min(to);
                if start < end {
                    total = total.saturating_add(seconds_between(start, end)?);
                }
            }
            date = date.next()?;
        }
        Ok(total)
    }
}

/// Every second, which is the clock crypto counts on (§5.2).
struct ContinuousClock;

impl SessionClock for ContinuousClock {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        seconds_between(from, to)
    }
}

fn seconds_between(from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
    to.secs()
        .checked_sub(from.secs())
        .and_then(|seconds| u64::try_from(seconds).ok())
        .ok_or(SpecError::ClockWentBackwards)
}

fn mark(when: &str, bid: &str) -> Step {
    quote(when, bid, MarketSession::Regular, true)
}

fn quote(when: &str, bid: &str, session: MarketSession, sane: bool) -> Step {
    Step {
        at: at(when),
        session,
        input: Input::Mark {
            bid: price(bid),
            sane,
        },
    }
}

fn tick(when: &str, session: MarketSession) -> Step {
    Step {
        at: at(when),
        session,
        input: Input::Clock,
    }
}

fn sale(when: &str, quantity: &str, at_price: &str, session: MarketSession) -> Step {
    Step {
        at: at(when),
        session,
        input: Input::Fill {
            side: Side::Sell,
            qty: qty(quantity),
            price: price(at_price),
        },
    }
}

fn new_day(when: &str, session: MarketSession) -> Step {
    Step {
        at: at(when),
        session,
        input: Input::RiskDayStarted,
    }
}

fn acknowledge(when: &str, restriction: Latch) -> Step {
    Step {
        at: at(when),
        session: MarketSession::Regular,
        input: Input::OwnerAcknowledged { restriction },
    }
}

fn allocate(when: &str, delta: &str) -> Step {
    Step {
        at: at(when),
        session: MarketSession::Regular,
        input: Input::AllocationChange {
            delta_usd: usd(delta),
        },
    }
}

fn loosen(when: &str, floor: &str, confirmed_at: &str, independent_approval: bool) -> Step {
    Step {
        at: at(when),
        session: MarketSession::Regular,
        input: Input::FloorLoosened {
            new_max_loss_from_allocation: SchemaDec::parse(floor, DecGrammar::OpenFraction)
                .expect("an open fraction"),
            confirmed_at: at(confirmed_at),
            independent_approval,
        },
    }
}

fn universe(when: &str, change: UniverseChange, reason: RemovalReason) -> Step {
    Step {
        at: at(when),
        session: MarketSession::Regular,
        input: Input::UniverseChanged {
            instrument: AssetId::parse(ASSET_A).expect("an asset id"),
            change,
            reason,
        },
    }
}

/// Opens a state and applies every step, returning one outcome per step.
///
/// A failure names the step it happened at, so a pending run says which input the fold stopped on rather
/// than only that it stopped.
fn walk(
    mandate: &ValidatedMandate,
    open: &Opening,
    clock: &dyn SessionClock,
    steps: &[Step],
) -> Result<Vec<Outcome>, String> {
    let mut state = RiskState::open(mandate, open, clock)
        .map_err(|e| format!("RiskState::open: {e} ({})", e.code()))?;
    let mut out = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        out.push(
            state
                .step(step)
                .map_err(|e| format!("step {}: {e} ({})", index.saturating_add(1), e.code()))?,
        );
    }
    Ok(out)
}

/// The outcome of one step by its position, one-based as the reference cases number them.
fn step_of(outcomes: &[Outcome], number: usize) -> Result<&Outcome, String> {
    outcomes
        .get(number.checked_sub(1).ok_or("steps are numbered from one")?)
        .ok_or_else(|| format!("there is no step {number}"))
}

/// Each event as text, written with this file's own words rather than the crate's spelling maps, so a
/// wrong spelling is `vocabulary.rs`'s business and a wrong *event* is this file's.
///
/// `action` is absent for the reason the module header gives.
fn trace(journal: &[RiskEvent]) -> Vec<String> {
    journal
        .iter()
        .map(|event| match event {
            RiskEvent::MandateVersionApplied { result } => match result {
                ApplyResult::Applied {
                    allocation_change,
                    max_loss_from_allocation,
                } => match (allocation_change, max_loss_from_allocation) {
                    (Some(delta), _) => format!("version applied, allocation {delta}"),
                    (None, Some(floor)) => format!("version applied, floor {}", floor.as_str()),
                    (None, None) => "version applied".to_owned(),
                },
                ApplyResult::Rejected { reason } => format!("version refused: {}", reason.code()),
            },
            RiskEvent::RiskDayStarted { day_start_equity } => {
                format!("day starts at {day_start_equity}")
            }
            RiskEvent::RiskLimitTriggered {
                limit,
                action,
                reason,
            } => match reason {
                Some(reason) => format!("triggered {limit:?} {action:?} because {reason:?}"),
                None => format!("triggered {limit:?} {action:?}"),
            },
            RiskEvent::RiskLimitLifted {
                limit,
                action,
                reason,
            } => match (action, reason) {
                (Some(action), Some(reason)) => {
                    format!("lifted {limit:?} {action:?} because {reason:?}")
                }
                (Some(action), None) => format!("lifted {limit:?} {action:?}"),
                (None, Some(reason)) => format!("lifted {limit:?} because {reason:?}"),
                (None, None) => format!("lifted {limit:?}"),
            },
            RiskEvent::HighWaterMarkReset { from, to } => format!("high water {from} -> {to}"),
            RiskEvent::AgentModeApplied { from, to } => format!("mode {from:?} -> {to:?}"),
            RiskEvent::KillSwitchActivated { scope, initiator } => {
                format!("kill switch {scope:?} for {initiator:?}")
            }
            RiskEvent::UniverseChanged { change, reason, .. } => {
                format!("universe {change:?} because {reason:?}")
            }
            RiskEvent::InstrumentRestrictionChanged {
                restriction,
                reason,
                active,
            } => format!("instrument {restriction:?} {active} because {reason:?}"),
            RiskEvent::GoalCompleted {
                reason,
                then,
                on_complete,
            } => format!("goal done {reason:?} {then:?} {on_complete:?}"),
            RiskEvent::PositionReleased { qty } => format!("released {qty}"),
            RiskEvent::AgentStopped {
                reason,
                loss_carry_usd,
            } => format!("stopped {reason:?} carrying {loss_carry_usd}"),
        })
        .collect()
}

fn restrictions(snapshot: &Snapshot) -> Vec<Restriction> {
    snapshot.restrictions.iter().copied().collect()
}

fn instrument_restrictions(snapshot: &Snapshot) -> Vec<InstrumentRestriction> {
    snapshot.instrument_restrictions.iter().copied().collect()
}

fn latched(snapshot: &Snapshot) -> Vec<LimitKey> {
    snapshot.latched.iter().copied().collect()
}

fn pending(outcome: &Outcome) -> Vec<LimitKey> {
    outcome.pending.iter().copied().collect()
}

fn rejection_code(outcome: &Outcome) -> Option<&'static str> {
    outcome.rejection.map(|r| r.code())
}

/// The scale rungs that are active, by index, without the durations beside them.
fn active(snapshot: &Snapshot) -> Vec<u8> {
    snapshot.active_rungs.keys().copied().collect()
}

/// The ladder `common::base()` carries, with two scale rungs so the size factor is a product of more
/// than one number: 0.02 at 0.75, 0.04 at 0.5, then `exits_only` at 0.06 and `flatten_and_pause` at
/// 0.08. The reference cases use this shape for the acknowledgment walk (MC-R09).
fn two_scale_rungs() -> Value {
    arr(vec![
        rung("0.02", "scale_sizes", Some("0.75")),
        rung("0.04", "scale_sizes", Some("0.5")),
        rung("0.06", "exits_only", None),
        rung("0.08", "flatten_and_pause", None),
    ])
}

/// The ladder scales at its first rung, holds through the hysteresis band, and latches the two stricter
/// rungs — with the lift boundary checked on both sides of a single dollar.
///
/// Recomputed from §5.5 and §5.2 with H = 10500 (100 shares marked at 105 on a 10000 allocation):
/// rung 0 (`at` 0.03) breaches at H − E ≥ 315, so a bid of 101.85 is exactly on it and the rule is `>=`.
/// It lifts only when H − E < (0.03 − 0.01) × H = 210, so a bid of 102.9 leaves H − E = 210 and does
/// **not** lift while 103 leaves 200 and does. Rung 1 (0.06) needs H − E ≥ 630 (a bid of 98.7) and rung 2
/// (0.08) needs 840 (96.6); neither reaches its 1.25× hard level (787.5 and 1050) at those bids, so both
/// arrive by plain confirmation with `breach_confirm_s` of 0.
#[test]
fn the_ladder_scales_and_the_hysteresis_band_lifts_only_on_the_far_side_of_its_boundary()
-> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "101.85"),
            mark("2026-09-21T14:03:00.000000000Z", "102.9"),
            mark("2026-09-21T14:04:00.000000000Z", "103"),
            mark("2026-09-21T14:05:00.000000000Z", "98.7"),
            mark("2026-09-21T14:06:00.000000000Z", "96.6"),
        ],
    )?;

    let first = step_of(&outcomes, 1)?;
    assert_eq!(first.snapshot.agent_equity, usd("10500"));
    assert_eq!(first.snapshot.high_water_mark, usd("10500"));
    assert_eq!(first.snapshot.drawdown, ratio("0"));
    assert_eq!(first.snapshot.size_factor, ratio("1"));
    assert!(
        trace(&first.journal).is_empty(),
        "a new high journals nothing"
    );

    let breached = step_of(&outcomes, 2)?;
    assert_eq!(breached.snapshot.drawdown, ratio("0.03"), "315 / 10500");
    assert_eq!(
        breached.snapshot.size_factor,
        ratio("0.5"),
        "the rung breaches exactly at its `at`, because the rule is `>=`"
    );
    assert_eq!(active(&breached.snapshot), vec![0]);
    assert_eq!(
        trace(&breached.journal),
        vec!["triggered DrawdownRung(0) ScaleSizes"]
    );

    let on_the_boundary = step_of(&outcomes, 3)?;
    assert_eq!(
        on_the_boundary.snapshot.size_factor,
        ratio("0.5"),
        "H - E = 210 is not < 210, so the rung is still active"
    );
    assert!(
        trace(&on_the_boundary.journal).is_empty(),
        "and nothing is journalled on the boundary"
    );

    let lifted = step_of(&outcomes, 4)?;
    assert_eq!(
        lifted.snapshot.size_factor,
        ratio("1"),
        "200 < 210 lifts it"
    );
    assert_eq!(active(&lifted.snapshot), Vec::<u8>::new());
    assert_eq!(
        trace(&lifted.journal),
        vec!["lifted DrawdownRung(0) ScaleSizes"]
    );

    let exits_only = step_of(&outcomes, 5)?;
    assert_eq!(exits_only.snapshot.drawdown, ratio("0.06"));
    assert_eq!(
        restrictions(&exits_only.snapshot),
        vec![Restriction::DrawdownExitsOnly]
    );
    assert_eq!(exits_only.snapshot.agent_mode, AgentMode::ExitsOnly);
    assert_eq!(
        latched(&exits_only.snapshot),
        vec![LimitKey::DrawdownRung(1)]
    );
    assert_eq!(
        trace(&exits_only.journal),
        vec![
            "triggered DrawdownRung(0) ScaleSizes",
            "triggered DrawdownRung(1) ExitsOnly",
            "mode Normal -> ExitsOnly",
        ]
    );

    let flatten = step_of(&outcomes, 6)?;
    assert_eq!(flatten.snapshot.drawdown, ratio("0.08"));
    assert_eq!(
        restrictions(&flatten.snapshot),
        vec![Restriction::DrawdownExitsOnly, Restriction::DrawdownFlatten]
    );
    assert_eq!(flatten.snapshot.agent_mode, AgentMode::Paused);
    assert_eq!(
        trace(&flatten.journal),
        vec![
            "triggered DrawdownRung(2) FlattenAndPause",
            "kill switch Agent for DrawdownRung(2)",
            "mode ExitsOnly -> Paused",
        ],
        "the flatten's kill switch comes with it, and the mode is applied last"
    );
    Ok(())
}

/// The size factor is the product of the active rungs' factors, and the folded field agrees with the
/// free function over the same rung set.
///
/// H = 10500, so rung 0 (0.02) arms at H − E ≥ 210 (a bid of 102.9) and rung 1 (0.04) at 420 (100.8).
/// 0.75 × 0.5 = 0.375 is recomputed by hand: no test may ask the crate what the product is.
#[test]
fn the_size_factor_is_the_product_of_the_active_rungs() -> Result<(), String> {
    let mandate = ladder_only(&[("/risk/drawdown_ladder", Some(two_scale_rungs()))])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "102.9"),
            mark("2026-09-21T14:03:00.000000000Z", "100.8"),
        ],
    )?;
    let ladder = &mandate.mandate().risk.drawdown_ladder;
    for (number, expected, rungs) in [
        (1, "1", Vec::new()),
        (2, "0.75", vec![0]),
        (3, "0.375", vec![0, 1]),
    ] {
        let snapshot = &step_of(&outcomes, number)?.snapshot;
        assert_eq!(
            snapshot.size_factor,
            ratio(expected),
            "step {number}: the product of the active rungs' factors"
        );
        assert_eq!(
            active(snapshot),
            rungs,
            "step {number}: which rungs are active"
        );
        assert_eq!(
            size_factor(ladder, &snapshot.active_rungs)
                .map_err(|e| format!("size_factor: {e} ({})", e.code()))?,
            snapshot.size_factor,
            "step {number}: the folded factor is the free function over the same rungs"
        );
    }
    Ok(())
}

/// A recovery shorter than `breach_confirm_s` does not restart confirmation (§5.6, planted bug 1).
///
/// H = 10500 and rung 1 breaches at H − E ≥ 630, so a bid of 98.5 is in breach and 99 is out. Breach
/// time is credited by the condition at each interval's **start**: 45 s in breach to 14:02:45, then a 5 s
/// recovery that is far shorter than the 60 s window, then 20 s more in breach — 65 s in total, so the
/// rung triggers at 14:03:10 and not before. An implementation that reset on the first quiet interval
/// would still be at 20 s there.
#[test]
fn a_short_recovery_does_not_restart_confirmation() -> Result<(), String> {
    let mandate = swing(&[
        ("/risk/max_daily_loss", Some(s("0.5"))),
        ("/risk/scale_lift_after_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "98.5"),
            mark("2026-09-21T14:02:45.000000000Z", "99"),
            mark("2026-09-21T14:02:50.000000000Z", "98.6"),
            mark("2026-09-21T14:03:10.000000000Z", "98.4"),
        ],
    )?;
    for number in 2..=4 {
        let outcome = step_of(&outcomes, number)?;
        assert_eq!(
            pending(outcome),
            vec![LimitKey::DrawdownRung(1)],
            "step {number}: breach time is accumulating and is shown as pending"
        );
        assert!(
            latched(&outcome.snapshot).is_empty(),
            "step {number}: 65 s have not passed yet, so nothing has latched"
        );
        assert_eq!(outcome.snapshot.agent_mode, AgentMode::Normal);
    }
    let triggered = step_of(&outcomes, 5)?;
    assert_eq!(
        trace(&triggered.journal),
        vec![
            "triggered DrawdownRung(1) ExitsOnly",
            "mode Normal -> ExitsOnly",
        ],
        "45 s + 20 s reaches the 60 s window, and plain confirmation carries no reason"
    );
    assert!(
        pending(triggered).is_empty(),
        "a limit that has triggered is no longer pending"
    );
    Ok(())
}

/// A recovery of at least `breach_confirm_s` does restart it, so the next breach needs the whole window
/// again (§5.6).
///
/// Out of breach from 14:02:30, and by 14:03:40 the condition has been false for 70 s — past the 60 s
/// window — so breach time is 0. Re-entering at 14:04:00 therefore triggers at 14:05:00 and not at
/// 14:04:30, which is where the 30 s already banked would have carried it.
#[test]
fn a_recovery_longer_than_the_window_restarts_confirmation() -> Result<(), String> {
    let mandate = swing(&[
        ("/risk/max_daily_loss", Some(s("0.5"))),
        ("/risk/scale_lift_after_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "98.5"),
            mark("2026-09-21T14:02:30.000000000Z", "99"),
            mark("2026-09-21T14:03:40.000000000Z", "99"),
            mark("2026-09-21T14:04:00.000000000Z", "98.6"),
            mark("2026-09-21T14:04:59.000000000Z", "98.6"),
            mark("2026-09-21T14:05:00.000000000Z", "98.6"),
        ],
    )?;
    assert!(
        step_of(&outcomes, 4)?.pending.is_empty(),
        "70 s out of breach clears the banked time, so nothing is pending any more"
    );
    let almost = step_of(&outcomes, 6)?;
    assert!(
        latched(&almost.snapshot).is_empty(),
        "59 s after re-entering is one second short of the window"
    );
    assert_eq!(almost.snapshot.agent_mode, AgentMode::Normal);
    let triggered = step_of(&outcomes, 7)?;
    assert_eq!(
        trace(&triggered.journal),
        vec![
            "triggered DrawdownRung(1) ExitsOnly",
            "mode Normal -> ExitsOnly",
        ],
        "the full 60 s is needed again, counted from the re-entry"
    );
    Ok(())
}

/// One flash print applies `exits_only` at once and latches nothing, and a sane quote below the level
/// clears it (§5.6, DEC-63, MI-4).
///
/// H = 10500, so rung 1's hard level is 1.25 × 0.06 × 10500 = 787.5 and rung 2's is 1050: a bid of 94.4
/// leaves H − E = 1060 and reaches both. Both rungs journal `hard_breach_pending` under one `hard_breach`
/// restriction, nothing is latched, and no kill switch fires. One second later a bid of 104 is below both
/// levels, so the restriction clears and the mode returns to normal — while breach time keeps its one
/// second, which is why both rungs are still pending.
#[test]
fn a_single_flash_print_latches_nothing_and_a_sane_quote_clears_the_hard_breach()
-> Result<(), String> {
    let mandate = swing(&[
        ("/risk/max_daily_loss", Some(s("0.5"))),
        ("/risk/scale_lift_after_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "94.4"),
            mark("2026-09-21T14:02:01.000000000Z", "104"),
        ],
    )?;
    let flash = step_of(&outcomes, 2)?;
    assert_eq!(
        restrictions(&flash.snapshot),
        vec![Restriction::HardBreach],
        "one print escalates to a temporary restriction and nothing else"
    );
    assert_eq!(flash.snapshot.agent_mode, AgentMode::ExitsOnly);
    assert!(
        latched(&flash.snapshot).is_empty(),
        "nothing latches on a single print"
    );
    assert_eq!(
        pending(flash),
        vec![LimitKey::DrawdownRung(1), LimitKey::DrawdownRung(2)]
    );
    assert_eq!(
        trace(&flash.journal),
        vec![
            "triggered DrawdownRung(0) ScaleSizes",
            "triggered DrawdownRung(1) ExitsOnly because HardBreachPending",
            "triggered DrawdownRung(2) ExitsOnly because HardBreachPending",
            "mode Normal -> ExitsOnly",
        ],
        "two rungs journal the pending hard breach under a single restriction"
    );

    let recovered = step_of(&outcomes, 3)?;
    assert!(
        restrictions(&recovered.snapshot).is_empty(),
        "a sane quote below the level clears it"
    );
    assert_eq!(recovered.snapshot.agent_mode, AgentMode::Normal);
    assert_eq!(
        trace(&recovered.journal),
        vec![
            "lifted DrawdownRung(0) ScaleSizes",
            "lifted DrawdownRung(1) because HardBreachCleared",
            "lifted DrawdownRung(2) because HardBreachCleared",
            "mode ExitsOnly -> Normal",
        ]
    );
    assert_eq!(
        pending(recovered),
        vec![LimitKey::DrawdownRung(1), LimitKey::DrawdownRung(2)],
        "the second of breach time is kept: clearing a hard breach is not a confirmation reset"
    );
    assert!(
        outcomes
            .iter()
            .flat_map(|o| o.journal.iter())
            .all(|e| !matches!(e, RiskEvent::KillSwitchActivated { .. })),
        "nothing was flattened by a print that lasted one second"
    );
    Ok(())
}

/// A hard breach latches only on a second sane quote at least min(`breach_confirm_s`, 10) s later
/// (§5.6, DEC-63, planted bug 2).
///
/// `breach_confirm_s` is 60, so the wait is 10 s. A second quote 9 s after the first is still **at** the
/// hard level — a bid of 94.3 is further past it than 94.4 was — and still latches nothing, and the whole
/// step journals nothing at all; the quote at exactly 10 s latches both rungs with `hard_trigger`,
/// flattens, and pauses.
#[test]
fn a_hard_breach_latches_only_on_a_second_quote_at_the_hard_wait() -> Result<(), String> {
    let mandate = swing(&[
        ("/risk/max_daily_loss", Some(s("0.5"))),
        ("/risk/scale_lift_after_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "94.4"),
            mark("2026-09-21T14:02:09.000000000Z", "94.3"),
            mark("2026-09-21T14:02:10.000000000Z", "94.2"),
        ],
    )?;
    let one_second_short = step_of(&outcomes, 3)?;
    assert_eq!(
        restrictions(&one_second_short.snapshot),
        vec![Restriction::HardBreach],
        "9 s is inside the wait, so the hard breach is still only pending"
    );
    assert!(latched(&one_second_short.snapshot).is_empty());
    assert!(
        trace(&one_second_short.journal).is_empty(),
        "a quote inside the wait changes nothing, so it journals nothing"
    );

    let latched_now = step_of(&outcomes, 4)?;
    assert_eq!(
        trace(&latched_now.journal),
        vec![
            "triggered DrawdownRung(1) ExitsOnly because HardTrigger",
            "triggered DrawdownRung(2) FlattenAndPause because HardTrigger",
            "kill switch Agent for DrawdownRung(2)",
            "mode ExitsOnly -> Paused",
        ],
        "at exactly the wait both rungs latch, and the `hard_breach` restriction is replaced"
    );
    assert_eq!(
        restrictions(&latched_now.snapshot),
        vec![Restriction::DrawdownExitsOnly, Restriction::DrawdownFlatten]
    );
    assert_eq!(
        latched(&latched_now.snapshot),
        vec![LimitKey::DrawdownRung(1), LimitKey::DrawdownRung(2)]
    );
    Ok(())
}

/// The effective mode is the strictest active restriction, and `AgentModeApplied` is journalled only
/// when it changes (§5.9, MI-6, planted bug 7).
///
/// E starts at 10000, so a bid of 98 is a 2% daily loss — exactly `max_daily_loss` — and latches
/// `daily_loss` at `exits_only`. A bid of 94 then adds the 0.06 rung, also `exits_only`, so the mode does
/// not move and nothing is journalled about it. The next risk day lifts the daily limit
/// (`daily_breach_min_s` is 0 here) and the mode still does not move, because the drawdown rung holds it.
/// One mode event in the whole walk is the assertion.
#[test]
fn the_strictest_restriction_holds_the_mode_and_only_a_change_is_journalled() -> Result<(), String>
{
    let mandate = swing(&[
        ("/risk/breach_confirm_s", Some(common::i(0))),
        ("/risk/daily_breach_min_s", Some(common::i(0))),
        ("/risk/scale_lift_after_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T04:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            new_day("2026-09-21T04:00:00.000000000Z", MarketSession::AfterHours),
            mark("2026-09-21T14:00:00.000000000Z", "98"),
            mark("2026-09-21T14:05:00.000000000Z", "94"),
            new_day("2026-09-22T04:00:00.000000000Z", MarketSession::AfterHours),
        ],
    )?;
    let daily = step_of(&outcomes, 2)?;
    assert_eq!(restrictions(&daily.snapshot), vec![Restriction::DailyLoss]);
    assert_eq!(daily.snapshot.agent_mode, AgentMode::ExitsOnly);
    assert_eq!(daily.snapshot.daily_pnl, usd("-200"));
    assert_eq!(daily.snapshot.daily_pnl_fraction, ratio("-0.02"));
    assert_eq!(
        trace(&daily.journal),
        vec![
            "triggered MaxDailyLoss ExitsOnly",
            "mode Normal -> ExitsOnly"
        ]
    );

    let both = step_of(&outcomes, 3)?;
    assert_eq!(
        restrictions(&both.snapshot),
        vec![Restriction::DailyLoss, Restriction::DrawdownExitsOnly]
    );
    assert_eq!(
        trace(&both.journal),
        vec![
            "triggered DrawdownRung(0) ScaleSizes",
            "triggered DrawdownRung(1) ExitsOnly"
        ],
        "two restrictions asking for the same mode change nothing, so no mode event"
    );

    let next_day = step_of(&outcomes, 4)?;
    assert_eq!(
        restrictions(&next_day.snapshot),
        vec![Restriction::DrawdownExitsOnly],
        "the daily limit lifts on the new day; the drawdown rung does not"
    );
    assert_eq!(next_day.snapshot.agent_mode, AgentMode::ExitsOnly);
    assert_eq!(next_day.snapshot.day_start_equity, usd("9400"));
    assert_eq!(
        trace(&next_day.journal),
        vec![
            "day starts at 9400",
            "lifted MaxDailyLoss",
            "instrument StaleMark true because NoSaneMark",
        ],
        "no mode event, because the strictest restriction is unchanged"
    );
    assert_eq!(
        outcomes
            .iter()
            .flat_map(|o| o.journal.iter())
            .filter(|e| matches!(e, RiskEvent::AgentModeApplied { .. }))
            .count(),
        1,
        "the mode changed exactly once in the whole walk"
    );
    Ok(())
}

/// The effective mode is the strictest restriction, not the last one applied (§5.9, MI-6, planted
/// bug 7).
///
/// The floor latches first and asks for `paused`; the goal then completes and asks for `exits_only`. An
/// implementation that took the mode from the restriction it added most recently would relax the agent
/// out of a flatten, which is the one direction §5.9 never allows. The order matters, so this is the
/// walk the reference cases do not carry: MC-R16 completes its goal *before* the floor.
#[test]
fn a_weaker_restriction_applied_later_never_relaxes_the_mode() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "90"),
            Step {
                at: at("2026-09-21T14:02:00.000000000Z"),
                session: MarketSession::Regular,
                input: Input::GoalComplete,
            },
        ],
    )?;
    assert_eq!(
        step_of(&outcomes, 1)?.snapshot.agent_mode,
        AgentMode::Paused
    );
    let completed = step_of(&outcomes, 2)?;
    assert_eq!(
        restrictions(&completed.snapshot),
        vec![
            Restriction::DrawdownExitsOnly,
            Restriction::DrawdownFlatten,
            Restriction::LifetimeFloor,
            Restriction::GoalComplete,
        ]
    );
    assert_eq!(
        completed.snapshot.agent_mode,
        AgentMode::Paused,
        "the floor still asks for paused, and the strictest wins"
    );
    assert_eq!(
        trace(&completed.journal),
        vec!["goal done None None Some(HoldProtected)"],
        "the mandate's `on_complete` is journalled, and the mode does not move, so nothing says it did"
    );
    Ok(())
}

/// Reported ratios round half to even at 12 places (§5.2, planted bug 9).
///
/// With H = 10500, a bid of 101.99 leaves H − E = 301 and 301 ÷ 10500 = 0.0286666…, whose 13th place is a
/// 6: half-even gives 0.028666666667 while truncation gives …666. A bid of 101 leaves 400 ÷ 10500 =
/// 0.0380952380952…, whose 13th place is a 2: half-even gives 0.038095238095 while rounding up would give
/// …096. The pair therefore separates half-even from both neighbours.
#[test]
fn reported_ratios_round_half_to_even_at_twelve_places() -> Result<(), String> {
    let mandate = one_change("/risk/max_daily_loss", s("0.5"))?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "101.99"),
            mark("2026-09-21T14:03:00.000000000Z", "101"),
        ],
    )?;
    assert_eq!(
        step_of(&outcomes, 2)?.snapshot.drawdown,
        ratio("0.028666666667"),
        "301 / 10500 rounds up at the 13th place"
    );
    assert_eq!(
        step_of(&outcomes, 3)?.snapshot.drawdown,
        ratio("0.038095238095"),
        "400 / 10500 rounds down at the 13th place"
    );
    assert_eq!(
        step_of(&outcomes, 3)?.snapshot.daily_pnl_fraction,
        ratio("0.01"),
        "an exact fraction keeps its own digits"
    );
    Ok(())
}

/// A breach still confirming at the rollover keeps confirming against the day it began in (§5.4,
/// planted bug 3).
///
/// The sale at 23:59:30 New York puts E at 9800 against a day-start equity of 10000, which is exactly the
/// 2% daily limit, so confirmation begins 30 s before midnight with a 60 s window. The rollover then sets
/// E₀ to 9800, against which the day's loss is **zero** — so the latch 40 s later can only have been
/// measured against the previous day's E₀, which is the whole of the rule.
#[test]
fn a_breach_pending_at_the_rollover_is_measured_against_the_day_it_began_in() -> Result<(), String>
{
    let mandate = swing(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T04:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            new_day("2026-09-21T04:00:00.000000000Z", MarketSession::AfterHours),
            sale(
                "2026-09-22T03:59:30.000000000Z",
                "100",
                "98",
                MarketSession::AfterHours,
            ),
            new_day("2026-09-22T04:00:00.000000000Z", MarketSession::AfterHours),
            tick("2026-09-22T04:00:40.000000000Z", MarketSession::AfterHours),
        ],
    )?;
    let before_midnight = step_of(&outcomes, 2)?;
    assert_eq!(before_midnight.snapshot.agent_equity, usd("9800"));
    assert_eq!(before_midnight.snapshot.daily_pnl_fraction, ratio("-0.02"));
    assert_eq!(
        pending(before_midnight),
        vec![LimitKey::MaxDailyLoss],
        "30 s of a 60 s window is not a breach yet"
    );

    let rollover = step_of(&outcomes, 3)?;
    assert_eq!(rollover.snapshot.day_start_equity, usd("9800"));
    assert!(
        pending(rollover).is_empty(),
        "the new day starts with no confirmation of its own"
    );
    assert_eq!(trace(&rollover.journal), vec!["day starts at 9800"]);

    let resolved = step_of(&outcomes, 4)?;
    assert_eq!(
        resolved.snapshot.daily_pnl,
        Usd::ZERO,
        "the new day has lost nothing"
    );
    assert_eq!(
        trace(&resolved.journal),
        vec![
            "triggered MaxDailyLoss ExitsOnly because ResolvedAtRollover",
            "mode Normal -> ExitsOnly",
        ],
        "so the 30 s before midnight plus 40 s after it can only be the previous day's breach"
    );
    assert_eq!(
        restrictions(&resolved.snapshot),
        vec![Restriction::DailyLoss]
    );
    Ok(())
}

/// A flash breach just before midnight is discarded once the condition has been false for the window
/// (§5.4).
///
/// The partial sale one second before midnight puts E at 9800 against 10000, and the sale one second
/// after it puts E back at 10000 — above the previous day's line as well as the new one. The carried
/// confirmation therefore never reaches its window and is dropped, and no later tick can revive it.
#[test]
fn a_flash_breach_before_midnight_is_discarded_after_the_rollover() -> Result<(), String> {
    let mandate = swing(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T04:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            new_day("2026-09-21T04:00:00.000000000Z", MarketSession::AfterHours),
            sale(
                "2026-09-22T03:59:59.000000000Z",
                "50",
                "96",
                MarketSession::AfterHours,
            ),
            new_day("2026-09-22T04:00:00.000000000Z", MarketSession::AfterHours),
            sale(
                "2026-09-22T04:00:01.000000000Z",
                "50",
                "104",
                MarketSession::AfterHours,
            ),
            tick("2026-09-22T04:01:02.000000000Z", MarketSession::AfterHours),
            tick("2026-09-22T04:30:00.000000000Z", MarketSession::AfterHours),
        ],
    )?;
    assert_eq!(step_of(&outcomes, 2)?.snapshot.agent_equity, usd("9800"));
    assert_eq!(
        pending(step_of(&outcomes, 2)?),
        vec![LimitKey::MaxDailyLoss]
    );
    assert_eq!(
        step_of(&outcomes, 4)?.snapshot.agent_equity,
        usd("10000"),
        "selling the rest above the bid puts equity back where it started"
    );
    for number in 3..=6 {
        let outcome = step_of(&outcomes, number)?;
        assert!(
            pending(outcome).is_empty(),
            "step {number}: the carried confirmation is not the new day's"
        );
        assert!(
            restrictions(&outcome.snapshot).is_empty(),
            "step {number}: a print that lasted two seconds latches nothing"
        );
    }
    assert!(
        outcomes
            .iter()
            .flat_map(|o| o.journal.iter())
            .all(|e| !matches!(e, RiskEvent::RiskLimitTriggered { .. })),
        "no limit triggered anywhere in the walk"
    );
    Ok(())
}

/// The daily lift needs both a new risk day and `daily_breach_min_s` since the breach (§5.4).
///
/// The breach lands at 23:30 New York, so the rollover half an hour later is not enough: the 3600 s
/// minimum is still 1800 s away. A tick one second short of it lifts nothing and the tick on it does.
#[test]
fn the_daily_lift_waits_for_the_minimum_as_well_as_the_new_day() -> Result<(), String> {
    let mandate = swing(&[("/risk/breach_confirm_s", Some(common::i(0)))])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T04:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            new_day("2026-09-21T04:00:00.000000000Z", MarketSession::AfterHours),
            sale(
                "2026-09-22T03:30:00.000000000Z",
                "100",
                "98",
                MarketSession::AfterHours,
            ),
            new_day("2026-09-22T04:00:00.000000000Z", MarketSession::AfterHours),
            tick("2026-09-22T04:29:59.000000000Z", MarketSession::AfterHours),
            tick("2026-09-22T04:30:00.000000000Z", MarketSession::AfterHours),
        ],
    )?;
    assert_eq!(
        trace(&step_of(&outcomes, 2)?.journal),
        vec![
            "triggered MaxDailyLoss ExitsOnly",
            "mode Normal -> ExitsOnly"
        ]
    );
    assert_eq!(
        restrictions(&step_of(&outcomes, 3)?.snapshot),
        vec![Restriction::DailyLoss],
        "a new risk day alone does not lift it"
    );
    assert_eq!(
        restrictions(&step_of(&outcomes, 4)?.snapshot),
        vec![Restriction::DailyLoss],
        "one second short of daily_breach_min_s does not either"
    );
    let lifted = step_of(&outcomes, 5)?;
    assert!(restrictions(&lifted.snapshot).is_empty());
    assert_eq!(
        trace(&lifted.journal),
        vec!["lifted MaxDailyLoss", "mode ExitsOnly -> Normal"],
        "the lift carries no reason, because nobody asked for it"
    );
    Ok(())
}

/// A confirmed breach on the new day renews the latch, and the lift waits for a risk day after **that**
/// (§5.4).
///
/// The renewal's day-start equity is 9800, and a bid of 95.6 leaves E at 9580 — a 2.2448…% loss, past the
/// 2% line, so the latch is renewed with `new_day_breach`. Five and a half hours later the minimum has
/// long passed and the limit is still held, because the renewal reset the wait for a new risk day.
#[test]
fn a_confirmed_new_day_breach_renews_the_latch() -> Result<(), String> {
    let mandate = swing(&[("/risk/breach_confirm_s", Some(common::i(0)))])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T04:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            new_day("2026-09-21T04:00:00.000000000Z", MarketSession::AfterHours),
            sale(
                "2026-09-22T03:30:00.000000000Z",
                "50",
                "96",
                MarketSession::AfterHours,
            ),
            new_day("2026-09-22T04:00:00.000000000Z", MarketSession::AfterHours),
            mark("2026-09-22T13:31:00.000000000Z", "95.6"),
            tick("2026-09-22T19:00:00.000000000Z", MarketSession::AfterHours),
        ],
    )?;
    let renewed = step_of(&outcomes, 4)?;
    assert_eq!(renewed.snapshot.agent_equity, usd("9580"));
    assert_eq!(renewed.snapshot.day_start_equity, usd("9800"));
    assert_eq!(
        renewed.snapshot.daily_pnl_fraction,
        ratio("-0.022448979592"),
        "-220 / 9800 rounded half to even at 12 places"
    );
    assert_eq!(
        trace(&renewed.journal),
        vec![
            "triggered DrawdownRung(0) ScaleSizes",
            "triggered MaxDailyLoss ExitsOnly because NewDayBreach",
        ],
        "the ladder is evaluated before the daily loss, and the mode does not move"
    );
    assert_eq!(
        restrictions(&step_of(&outcomes, 5)?.snapshot),
        vec![Restriction::DailyLoss],
        "the renewal starts the wait for a new risk day again, so hours do not lift it"
    );
    Ok(())
}

/// A daily `flatten_and_pause` is acknowledged only once the agent is flat, and the acknowledgment leaves
/// `exits_only` until the next risk day (§5.4).
#[test]
fn a_daily_flatten_acknowledged_after_flat_leaves_exits_only_until_the_next_day()
-> Result<(), String> {
    let mandate = swing(&[
        ("/risk/breach_confirm_s", Some(common::i(0))),
        ("/risk/daily_loss_action", Some(s("flatten_and_pause"))),
        ("/risk/daily_breach_min_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T04:00:00.000000000Z", "20", "150", "0"),
        &clock,
        &[
            new_day("2026-09-21T04:00:00.000000000Z", MarketSession::AfterHours),
            mark("2026-09-21T14:00:00.000000000Z", "140"),
            acknowledge("2026-09-21T14:10:00.000000000Z", Latch::DailyLoss),
            sale(
                "2026-09-21T14:20:00.000000000Z",
                "20",
                "140",
                MarketSession::Regular,
            ),
            acknowledge("2026-09-21T14:30:00.000000000Z", Latch::DailyLoss),
            new_day("2026-09-22T04:00:00.000000000Z", MarketSession::AfterHours),
        ],
    )?;
    let flattened = step_of(&outcomes, 2)?;
    assert_eq!(flattened.snapshot.agent_mode, AgentMode::Paused);
    assert_eq!(
        trace(&flattened.journal),
        vec![
            "triggered MaxDailyLoss FlattenAndPause",
            "kill switch Agent for MaxDailyLoss",
            "mode Normal -> Paused",
        ]
    );
    let too_early = step_of(&outcomes, 3)?;
    assert_eq!(
        rejection_code(too_early),
        Some("flatten_in_progress"),
        "the agent still holds 20 shares"
    );
    assert_eq!(too_early.snapshot.agent_mode, AgentMode::Paused);
    assert!(trace(&too_early.journal).is_empty());

    let acknowledged = step_of(&outcomes, 5)?;
    assert_eq!(rejection_code(acknowledged), None);
    assert_eq!(
        restrictions(&acknowledged.snapshot),
        vec![Restriction::DailyLoss],
        "the restriction stays; only the mode it asks for changes"
    );
    assert_eq!(acknowledged.snapshot.agent_mode, AgentMode::ExitsOnly);
    assert_eq!(
        trace(&acknowledged.journal),
        vec!["mode Paused -> ExitsOnly"]
    );

    let next_day = step_of(&outcomes, 6)?;
    assert!(restrictions(&next_day.snapshot).is_empty());
    assert_eq!(
        trace(&next_day.journal),
        vec![
            "day starts at 9800",
            "lifted MaxDailyLoss",
            "mode ExitsOnly -> Normal",
        ]
    );
    Ok(())
}

/// Acknowledgment is refused while a flatten is unfinished; afterwards H becomes E, the latched rungs
/// lift, and the scale rungs return one at a time, highest `at` first (§5.8, planted bug 10).
///
/// The ladder here has two scale rungs, so the factor is 0.75 × 0.5 = 0.375 while both are active. After
/// the reset the higher rung (0.04) lifts first, 600 s of regular-session time later; the lower one
/// (0.02) starts its own 600 s only then, so a tick 300 s after the first lift changes nothing.
#[test]
fn acknowledgment_waits_for_flat_then_resets_the_high_water_mark_and_lifts_rungs_in_turn()
-> Result<(), String> {
    let mandate = swing(&[
        ("/risk/drawdown_ladder", Some(two_scale_rungs())),
        ("/risk/breach_confirm_s", Some(common::i(0))),
        ("/risk/max_daily_loss", Some(s("0.5"))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "96.6"),
            acknowledge("2026-09-21T14:03:00.000000000Z", Latch::DrawdownLadder),
            sale(
                "2026-09-21T14:04:00.000000000Z",
                "100",
                "96.5",
                MarketSession::Regular,
            ),
            acknowledge("2026-09-21T14:10:00.000000000Z", Latch::DrawdownLadder),
            tick("2026-09-21T14:20:00.000000000Z", MarketSession::Regular),
            tick("2026-09-21T14:25:00.000000000Z", MarketSession::Regular),
            tick("2026-09-21T14:30:00.000000000Z", MarketSession::Regular),
        ],
    )?;
    let all_four = step_of(&outcomes, 2)?;
    assert_eq!(all_four.snapshot.size_factor, ratio("0.375"));
    assert_eq!(all_four.snapshot.agent_mode, AgentMode::Paused);
    assert_eq!(
        trace(&all_four.journal),
        vec![
            "triggered DrawdownRung(0) ScaleSizes",
            "triggered DrawdownRung(1) ScaleSizes",
            "triggered DrawdownRung(2) ExitsOnly",
            "triggered DrawdownRung(3) FlattenAndPause",
            "kill switch Agent for DrawdownRung(3)",
            "mode Normal -> Paused",
        ]
    );

    let while_flattening = step_of(&outcomes, 3)?;
    assert_eq!(
        rejection_code(while_flattening),
        Some("flatten_in_progress")
    );
    assert_eq!(
        while_flattening.snapshot.high_water_mark,
        usd("10500"),
        "a refused acknowledgment resets nothing"
    );
    assert!(trace(&while_flattening.journal).is_empty());

    let reset = step_of(&outcomes, 5)?;
    assert_eq!(rejection_code(reset), None);
    assert_eq!(reset.snapshot.high_water_mark, usd("9650"));
    assert_eq!(reset.snapshot.drawdown, ratio("0"));
    assert!(latched(&reset.snapshot).is_empty());
    assert!(restrictions(&reset.snapshot).is_empty());
    assert_eq!(
        reset.snapshot.size_factor,
        ratio("0.375"),
        "the scale rungs stay active: sizes return in steps, not at once"
    );
    assert_eq!(
        trace(&reset.journal),
        vec![
            "high water 10500 -> 9650",
            "lifted DrawdownRung(2) because OwnerAcknowledged",
            "lifted DrawdownRung(3) because OwnerAcknowledged",
            "mode Paused -> Normal",
        ]
    );

    let first_lift = step_of(&outcomes, 6)?;
    assert_eq!(
        first_lift.snapshot.size_factor,
        ratio("0.75"),
        "the 0.04 rung lifts first, leaving the 0.02 rung's 0.75"
    );
    assert_eq!(active(&first_lift.snapshot), vec![0]);
    assert_eq!(
        trace(&first_lift.journal),
        vec!["lifted DrawdownRung(1) ScaleSizes"]
    );

    let halfway = step_of(&outcomes, 7)?;
    assert_eq!(
        halfway.snapshot.size_factor,
        ratio("0.75"),
        "the lower rung's own 600 s started at the first lift, not at the reset"
    );
    assert!(trace(&halfway.journal).is_empty());

    let second_lift = step_of(&outcomes, 8)?;
    assert_eq!(second_lift.snapshot.size_factor, ratio("1"));
    assert_eq!(
        trace(&second_lift.journal),
        vec!["lifted DrawdownRung(0) ScaleSizes"]
    );
    Ok(())
}

/// The lifetime floor sits at C × (1 − f) + L, so the inherited loss raises it, and it cannot be
/// acknowledged (§5.7, MI-3).
///
/// With C = 10000 and f = 0.1 the floor is 9000; an inherited loss of 300 puts it at 9300, which a bid of
/// 93 reaches exactly and a bid of 97 does not. The second walk is the same marks with no inherited loss,
/// where nothing latches — so the test pins that L is read rather than that 9300 happens to be a floor.
#[test]
fn the_floor_is_raised_by_the_inherited_loss_and_cannot_be_acknowledged() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let steps = [
        mark("2026-09-21T14:01:00.000000000Z", "97"),
        mark("2026-09-21T14:02:00.000000000Z", "93"),
        acknowledge("2026-09-21T14:03:00.000000000Z", Latch::LifetimeFloor),
    ];
    let carried = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "300"),
        &clock,
        &steps,
    )?;
    assert_eq!(
        step_of(&carried, 1)?.snapshot.inherited_loss,
        usd("300"),
        "the connection's loss carry is the agent's opening inherited loss"
    );
    assert!(
        latched(&step_of(&carried, 1)?.snapshot).is_empty(),
        "9700 is above the 9300 floor"
    );
    let at_the_floor = step_of(&carried, 2)?;
    assert_eq!(
        trace(&at_the_floor.journal),
        vec![
            "triggered DrawdownRung(1) ExitsOnly",
            "triggered LifetimeFloor FlattenAndPause",
            "kill switch Agent for LifetimeFloor",
            "mode Normal -> Paused",
        ],
        "the floor is evaluated after the ladder and flattens on confirmation"
    );
    assert_eq!(
        restrictions(&at_the_floor.snapshot),
        vec![Restriction::DrawdownExitsOnly, Restriction::LifetimeFloor]
    );
    let refused = step_of(&carried, 3)?;
    assert_eq!(rejection_code(refused), Some("not_acknowledgeable"));
    assert!(
        refused.snapshot.latched.contains(&LimitKey::LifetimeFloor),
        "and the floor is still latched afterwards"
    );
    assert!(trace(&refused.journal).is_empty());

    let uncarried = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &steps,
    )?;
    assert!(
        !step_of(&uncarried, 2)?
            .snapshot
            .latched
            .contains(&LimitKey::LifetimeFloor),
        "without the inherited loss the floor is 9000, which 9300 does not reach"
    );
    Ok(())
}

/// The floor lifts only on a version that loosens it, after a full risk day when one user asks (§5.7).
///
/// The confirmation is at 14:01 on 2026-09-21, so its risk day ends at 00:00 on the 22nd and the first
/// **full** day after it ends at 00:00 on the 23rd: an attempt the same afternoon is `waiting_period`. An
/// independent approver skips the wait, but a version that does not raise `max_loss_from_allocation` is
/// `not_loosening`. Raising it to 0.2 moves the floor to 8000, below E, so the floor lifts —
/// while the drawdown flatten it arrived with does not, which is why the mode does not move.
#[test]
fn loosening_a_latched_floor_waits_a_full_risk_day_and_must_actually_loosen() -> Result<(), String>
{
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "90"),
            loosen(
                "2026-09-21T14:02:00.000000000Z",
                "0.2",
                "2026-09-21T14:01:00.000000000Z",
                false,
            ),
            loosen(
                "2026-09-21T14:03:00.000000000Z",
                "0.1",
                "2026-09-21T14:01:00.000000000Z",
                true,
            ),
            loosen(
                "2026-09-21T14:04:00.000000000Z",
                "0.2",
                "2026-09-21T14:01:00.000000000Z",
                true,
            ),
        ],
    )?;
    let waiting = step_of(&outcomes, 2)?;
    assert_eq!(rejection_code(waiting), Some("waiting_period"));
    assert_eq!(
        trace(&waiting.journal),
        vec!["version refused: waiting_period"],
        "a refused version is journalled, so a caller cannot lose the refusal"
    );
    assert_eq!(
        rejection_code(step_of(&outcomes, 3)?),
        Some("not_loosening")
    );

    let lifted = step_of(&outcomes, 4)?;
    assert_eq!(rejection_code(lifted), None);
    assert!(
        !lifted.snapshot.latched.contains(&LimitKey::LifetimeFloor),
        "the floor lifts on the loosening version"
    );
    assert_eq!(
        restrictions(&lifted.snapshot),
        vec![Restriction::DrawdownExitsOnly, Restriction::DrawdownFlatten],
        "and nothing else lifts with it"
    );
    assert_eq!(lifted.snapshot.agent_mode, AgentMode::Paused);
    assert_eq!(
        trace(&lifted.journal),
        vec![
            "version applied, floor 0.2",
            "lifted LifetimeFloor because VersionLoosened",
        ]
    );
    Ok(())
}

/// A version that leaves equity at or below the new floor is refused (§5.7).
///
/// A bid of 80 puts E at 8000, under the 9000 floor. Raising f to 0.15 moves the floor to 8500, which E is
/// still below, so the version is `still_below_new_floor`; 0.25 moves it to 7500 and the floor lifts.
#[test]
fn a_version_that_leaves_equity_below_the_new_floor_is_refused() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "80"),
            loosen(
                "2026-09-21T14:02:00.000000000Z",
                "0.15",
                "2026-09-21T14:01:00.000000000Z",
                true,
            ),
            loosen(
                "2026-09-21T14:03:00.000000000Z",
                "0.25",
                "2026-09-21T14:01:00.000000000Z",
                true,
            ),
        ],
    )?;
    assert!(
        step_of(&outcomes, 1)?
            .snapshot
            .latched
            .contains(&LimitKey::LifetimeFloor)
    );
    assert_eq!(
        rejection_code(step_of(&outcomes, 2)?),
        Some("still_below_new_floor"),
        "8000 is below the 8500 the new fraction would allow"
    );
    let lifted = step_of(&outcomes, 3)?;
    assert_eq!(rejection_code(lifted), None);
    assert!(!lifted.snapshot.latched.contains(&LimitKey::LifetimeFloor));
    Ok(())
}

/// The floor a loosening version must clear is C × (1 − f′) **+ L**, so the inherited loss decides the
/// answer (§5.7, MI-14, planted bug 14).
///
/// C = 10000 and L = 500, so the floor starts at 9000 + 500 = 9500 and a bid of 85 latches it at E = 8500.
/// Raising f to 0.2 moves C × (1 − f′) to 8000 — which E clears — but the floor to 8500, which it does not,
/// so the version is `still_below_new_floor`. An implementation that dropped L would have lifted the floor
/// there, which is the one direction §5.7 never allows. 0.25 moves the floor to 8000 and does lift it.
#[test]
fn a_loosening_version_must_clear_the_floor_the_inherited_loss_raises() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "500"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "85"),
            loosen(
                "2026-09-21T14:02:00.000000000Z",
                "0.2",
                "2026-09-21T14:01:00.000000000Z",
                true,
            ),
            loosen(
                "2026-09-21T14:03:00.000000000Z",
                "0.25",
                "2026-09-21T14:01:00.000000000Z",
                true,
            ),
        ],
    )?;
    let latched_floor = step_of(&outcomes, 1)?;
    assert_eq!(latched_floor.snapshot.agent_equity, usd("8500"));
    assert_eq!(latched_floor.snapshot.inherited_loss, usd("500"));
    assert!(
        latched_floor
            .snapshot
            .latched
            .contains(&LimitKey::LifetimeFloor),
        "8500 is below the 9500 that C x (1 - 0.1) + L allows"
    );
    assert_eq!(
        rejection_code(step_of(&outcomes, 2)?),
        Some("still_below_new_floor"),
        "f' of 0.2 puts C x (1 - f') at 8000, which E clears, and the floor at 8500, which it does not"
    );
    assert!(
        step_of(&outcomes, 2)?
            .snapshot
            .latched
            .contains(&LimitKey::LifetimeFloor),
        "and a refused version lifts nothing"
    );
    let lifted = step_of(&outcomes, 3)?;
    assert_eq!(rejection_code(lifted), None);
    assert!(!lifted.snapshot.latched.contains(&LimitKey::LifetimeFloor));
    assert_eq!(
        trace(&lifted.journal),
        vec![
            "version applied, floor 0.25",
            "lifted LifetimeFloor because VersionLoosened",
        ]
    );
    Ok(())
}

/// The loss carried to the connection is max(0, net contributed − E), so withdrawing first cannot shrink
/// it (§5.7, MI-14, planted bug 6).
///
/// The agent loses 850 and goes flat, and then either retires or withdraws 9000 and retires. Net
/// contributed falls from 10000 to 1000 and equity from 9150 to 150, so the difference is 850 either way —
/// which `C − E` would not be, since the capital base is scaled by the withdrawal and E is not.
#[test]
fn a_withdrawal_cannot_shrink_the_loss_carried_to_the_connection() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let losing = [
        mark("2026-09-21T14:01:00.000000000Z", "91.5"),
        sale(
            "2026-09-21T14:02:00.000000000Z",
            "100",
            "91.5",
            MarketSession::Regular,
        ),
    ];
    let straight: Vec<Step> = losing
        .iter()
        .cloned()
        .chain([retire("2026-09-21T14:03:00.000000000Z")])
        .collect();
    let withdrawn: Vec<Step> = losing
        .iter()
        .cloned()
        .chain([
            allocate("2026-09-21T14:03:00.000000000Z", "-9000"),
            retire("2026-09-21T14:04:00.000000000Z"),
        ])
        .collect();
    let open = patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0");

    let plain = walk(&mandate, &open, &clock, &straight)?;
    let retired = step_of(&plain, 3)?;
    assert_eq!(retired.snapshot.agent_equity, usd("9150"));
    assert_eq!(retired.snapshot.net_contributed, usd("10000"));
    assert_eq!(
        trace(&retired.journal),
        vec!["stopped OwnerStop carrying 850", "mode Paused -> Stopped"],
        "10000 contributed less 9150 of equity"
    );

    let after_withdrawal = walk(&mandate, &open, &clock, &withdrawn)?;
    let scaled = step_of(&after_withdrawal, 3)?;
    assert_eq!(scaled.snapshot.agent_equity, usd("150"));
    assert_eq!(scaled.snapshot.net_contributed, usd("1000"));
    let stopped = step_of(&after_withdrawal, 4)?;
    assert_eq!(
        trace(&stopped.journal),
        vec!["stopped OwnerStop carrying 850", "mode Paused -> Stopped"],
        "the carry is the same as without the withdrawal, because both sides fell by 9000"
    );
    assert!(
        stopped
            .snapshot
            .restrictions
            .contains(&Restriction::Retired)
    );
    assert_eq!(stopped.snapshot.agent_mode, AgentMode::Stopped);
    Ok(())
}

/// `would_trigger_limit` tests the 1.25× hard levels as well as the soft ones (§5.1, planted bug 5).
///
/// The guard only ever bites within 10⁻¹² of a threshold, because §5.1 scales H, E₀, C, and L by exactly
/// k = (E + Δ) ÷ E and the only thing that can make a condition newly true is the ceiling's residue. This
/// is a position that reaches it. 74.999999985 shares bought at 100 marked at 89.999999998 give
/// E = 9250.00000000000000003 against H = 10000, so H − E = 749.99999999999999997: rung 1's soft level
/// (0.06 × 10000 = 600) is true and its **hard** level (1.25 × 0.06 × 10000 = 750) is false by three parts
/// in 10²⁰. Withdrawing a single dollar makes H′ = ceil(10000 × 9249.00000000000000003 ÷
/// 9250.00000000000000003, 12) = 9998.918918918919, whose hard level 749.918918918918925 the scaled gap
/// 749.91891891891899997 does reach — so the answer is `would_trigger_limit`, and an implementation that
/// checked only the soft levels would apply the change. `reference/mandate/ref.py` returns
/// `would_trigger_limit` here, and the same port with the hard levels dropped applies it.
#[test]
fn would_trigger_limit_tests_the_hard_levels_and_not_only_the_soft_ones() -> Result<(), String> {
    let mandate = swing(&[("/risk/max_daily_loss", Some(s("0.5")))])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "74.999999985", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "89.999999998"),
            allocate("2026-09-21T14:01:10.000000000Z", "-1"),
        ],
    )?;
    let marked = step_of(&outcomes, 1)?;
    assert_eq!(
        marked.snapshot.agent_equity,
        usd("9250.00000000000000003"),
        "the position is chosen so that the gap sits just inside the hard level"
    );
    assert_eq!(marked.snapshot.high_water_mark, usd("10000"));
    assert!(
        latched(&marked.snapshot).is_empty(),
        "breach_confirm_s is 60 here, so the soft breach has not confirmed and nothing is latched"
    );
    let refused = step_of(&outcomes, 2)?;
    assert_eq!(
        rejection_code(refused),
        Some("would_trigger_limit"),
        "the soft level was already true, so only the hard level can have become newly true"
    );
    assert_eq!(
        refused.snapshot.high_water_mark,
        usd("10000"),
        "and the refused change scaled nothing: 9998.918918918919 is what applying it would have given"
    );
    assert_eq!(
        trace(&refused.journal),
        vec!["version refused: would_trigger_limit"]
    );
    Ok(())
}

/// An allocation increase is refused while any limit is latched, and a decrease below the agent's gross
/// exposure is refused too (§5.1, MI-7).
#[test]
fn an_increase_is_refused_while_latched_and_a_decrease_below_exposure_is_refused()
-> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "105"),
            mark("2026-09-21T14:02:00.000000000Z", "98.7"),
            allocate("2026-09-21T14:03:00.000000000Z", "1000"),
            allocate("2026-09-21T14:04:00.000000000Z", "-1000"),
        ],
    )?;
    assert_eq!(
        latched(&step_of(&outcomes, 2)?.snapshot),
        vec![LimitKey::DrawdownRung(1)]
    );
    let increase = step_of(&outcomes, 3)?;
    assert_eq!(
        rejection_code(increase),
        Some("increase_blocked_while_latched")
    );
    assert_eq!(
        trace(&increase.journal),
        vec!["version refused: increase_blocked_while_latched"]
    );
    assert_eq!(
        increase.snapshot.capital_base,
        usd("10000"),
        "a refused change scales nothing"
    );
    assert_eq!(increase.snapshot.net_contributed, usd("10000"));

    let decrease = step_of(&outcomes, 4)?;
    assert_eq!(
        rejection_code(decrease),
        Some("equity_below_exposure"),
        "8870 would not cover a 9870 position"
    );
    assert_eq!(decrease.snapshot.capital_base, usd("10000"));
    Ok(())
}

/// An applied allocation change scales H, E₀, C, and L by (E + Δ) ÷ E rounded **up** at 12 places, so every
/// ratio it reports is preserved and no limit is armed or lifted (§5.1, MI-2, planted bug 4).
///
/// E = 9500 against H = 10000, a 5% drawdown. Withdrawing 5000 gives k = 4500 ÷ 9500 and
/// H′ = ceil(10000 × 4500 ÷ 9500, 12) = ceil(4736.842105263157894…) = 4736.842105263158, which keeps the
/// drawdown at 5% rather than letting the withdrawal lower it. Putting the 5000 back gives
/// ceil(4736.842105263158 × 9500 ÷ 4500, 12) = ceil(10000.000000000000222…) = 10000.000000000001 — strictly
/// **above** the 10000 it started at, which is the direction MI-2 needs and which rounding half to even
/// would have returned to exactly 10000.
#[test]
fn an_applied_allocation_change_scales_the_marks_upward_and_arms_nothing() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "10", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "50"),
            allocate("2026-09-21T14:02:00.000000000Z", "-5000"),
            allocate("2026-09-21T14:03:00.000000000Z", "5000"),
        ],
    )?;
    let before = step_of(&outcomes, 1)?;
    assert_eq!(before.snapshot.agent_equity, usd("9500"));
    assert_eq!(before.snapshot.high_water_mark, usd("10000"));
    assert_eq!(before.snapshot.drawdown, ratio("0.05"));
    assert_eq!(before.snapshot.daily_pnl_fraction, ratio("-0.05"));

    let withdrawn = step_of(&outcomes, 2)?;
    assert_eq!(rejection_code(withdrawn), None);
    assert_eq!(withdrawn.snapshot.agent_equity, usd("4500"));
    assert_eq!(withdrawn.snapshot.high_water_mark, usd("4736.842105263158"));
    assert_eq!(
        withdrawn.snapshot.day_start_equity,
        usd("4736.842105263158")
    );
    assert_eq!(withdrawn.snapshot.capital_base, usd("4736.842105263158"));
    assert_eq!(withdrawn.snapshot.daily_pnl, usd("-236.842105263158"));
    assert_eq!(
        withdrawn.snapshot.drawdown,
        ratio("0.05"),
        "the withdrawal neither lowers the drawdown nor raises it"
    );
    assert_eq!(withdrawn.snapshot.daily_pnl_fraction, ratio("-0.05"));
    assert_eq!(withdrawn.snapshot.net_contributed, usd("5000"));
    assert_eq!(
        trace(&withdrawn.journal),
        vec!["version applied, allocation -5000"],
        "a change that arms nothing journals nothing but itself"
    );

    let restored = step_of(&outcomes, 3)?;
    assert_eq!(rejection_code(restored), None);
    assert_eq!(restored.snapshot.agent_equity, usd("9500"));
    assert_eq!(
        restored.snapshot.high_water_mark,
        usd("10000.000000000001"),
        "one rounding upward each way, so the round trip never comes back lower"
    );
    assert_eq!(restored.snapshot.capital_base, usd("10000.000000000001"));
    assert_eq!(restored.snapshot.daily_pnl, usd("-500.000000000001"));
    assert_eq!(restored.snapshot.drawdown, ratio("0.05"));
    assert_eq!(restored.snapshot.net_contributed, usd("10000"));
    assert!(
        restored.snapshot.high_water_mark > before.snapshot.high_water_mark,
        "the conservative direction: the scaled mark is at or above the exact value"
    );
    assert_eq!(
        trace(&restored.journal),
        vec!["version applied, allocation 5000"]
    );
    assert_eq!(
        active(&restored.snapshot),
        vec![0],
        "the rung that was active before the pair is the rung that is active after it"
    );
    Ok(())
}

/// Only regular-session sane marks move an equity's E, staleness is measured in regular-session time, and
/// a failed mark sets the per-instrument restriction (§5.2, planted bug 8).
///
/// The pre-market bid of 80 would put E at 8000, under the 9000 lifetime floor — so an implementation that
/// used extended-hours quotes would latch the floor here. The staleness clock is regular-session seconds:
/// between the 15:59:30 mark and the 08:00 pre-market print only 30 s of regular session pass, which is why
/// nothing is stale despite sixteen hours of wall clock.
#[test]
fn extended_hours_marks_are_ignored_and_staleness_counts_regular_session_seconds()
-> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &opening("2026-09-21T13:31:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T13:32:00.000000000Z", "100"),
            mark("2026-09-21T19:59:30.000000000Z", "100"),
            quote(
                "2026-09-22T12:00:00.000000000Z",
                "80",
                MarketSession::PreMarket,
                true,
            ),
            quote(
                "2026-09-22T13:31:00.000000000Z",
                "99",
                MarketSession::Regular,
                false,
            ),
            mark("2026-09-22T13:32:00.000000000Z", "99"),
            tick("2026-09-22T13:34:00.000000000Z", MarketSession::Regular),
        ],
    )?;
    let extended = step_of(&outcomes, 3)?;
    assert_eq!(
        extended.snapshot.agent_equity,
        usd("10000"),
        "an extended-hours quote does not move an equity's E"
    );
    assert!(
        latched(&extended.snapshot).is_empty(),
        "so the floor a bid of 80 would have breached is never reached"
    );
    assert!(
        instrument_restrictions(&extended.snapshot).is_empty(),
        "30 s of regular session is nowhere near the 120 s limit"
    );
    assert!(trace(&extended.journal).is_empty());

    let failed = step_of(&outcomes, 4)?;
    assert_eq!(
        instrument_restrictions(&failed.snapshot),
        vec![InstrumentRestriction::StaleMark],
        "a mark that fails its checks is a stale mark, whatever the timer says"
    );
    assert_eq!(failed.snapshot.agent_equity, usd("10000"));
    assert!(
        restrictions(&failed.snapshot).is_empty(),
        "and it restricts the instrument, never the agent"
    );
    assert_eq!(
        trace(&failed.journal),
        vec!["instrument StaleMark true because NoSaneMark"]
    );

    let recovered = step_of(&outcomes, 5)?;
    assert_eq!(recovered.snapshot.agent_equity, usd("9900"));
    assert!(instrument_restrictions(&recovered.snapshot).is_empty());
    assert_eq!(
        trace(&recovered.journal),
        vec!["instrument StaleMark false because SaneMark"]
    );

    let gone_quiet = step_of(&outcomes, 6)?;
    assert_eq!(
        instrument_restrictions(&gone_quiet.snapshot),
        vec![InstrumentRestriction::StaleMark],
        "120 s of regular session with no sane mark is the limit itself"
    );
    assert_eq!(
        trace(&gone_quiet.journal),
        vec!["instrument StaleMark true because NoSaneMark"]
    );
    assert!(
        outcomes
            .iter()
            .all(|o| o.snapshot.agent_mode == AgentMode::Normal),
        "a stale mark never moves the agent's mode (MI-1)"
    );
    Ok(())
}

/// A removed instrument is restricted in that instrument only, and re-admission clears it with the reason
/// that re-admitted it (§5.9, §2.3, MI-1).
#[test]
fn a_removed_instrument_is_restricted_alone_and_re_admission_clears_it() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "100"),
            universe(
                "2026-09-21T14:02:00.000000000Z",
                UniverseChange::Removed,
                RemovalReason::ThesisExpired,
            ),
            mark("2026-09-21T14:03:00.000000000Z", "100"),
            universe(
                "2026-09-21T14:04:00.000000000Z",
                UniverseChange::Admitted,
                RemovalReason::ThesisAdmitted,
            ),
        ],
    )?;
    let removed = step_of(&outcomes, 2)?;
    assert_eq!(
        instrument_restrictions(&removed.snapshot),
        vec![InstrumentRestriction::RemovedInstrument]
    );
    assert_eq!(
        trace(&removed.journal),
        vec![
            "universe Removed because ThesisExpired",
            "instrument RemovedInstrument true because Removal(ThesisExpired)",
        ],
        "one event per restriction that changed, carrying the reason that removed it"
    );
    assert_eq!(
        instrument_restrictions(&step_of(&outcomes, 3)?.snapshot),
        vec![InstrumentRestriction::RemovedInstrument],
        "a mark does not re-admit anything"
    );
    let admitted = step_of(&outcomes, 4)?;
    assert!(instrument_restrictions(&admitted.snapshot).is_empty());
    assert_eq!(
        trace(&admitted.journal),
        vec![
            "universe Admitted because ThesisAdmitted",
            "instrument RemovedInstrument false because Removal(ThesisAdmitted)",
        ]
    );
    assert!(
        outcomes
            .iter()
            .all(|o| restrictions(&o.snapshot).is_empty()
                && o.snapshot.agent_mode == AgentMode::Normal),
        "the agent is never restricted by one instrument leaving its universe"
    );
    Ok(())
}

/// A `profit_stop` is confirmed by time in breach inside the risk state, with no hard trigger (§3.1, §5.6).
///
/// The level is E − C ≥ 0.1 × C = 1000, so E ≥ 11000. Thirty seconds above it, a ten-second dip that is
/// far shorter than the 60 s window, then thirty more: the goal completes at 14:02:10 with the one outcome
/// §3.1 gives a `profit_stop`, and nothing about it is a `RiskLimitTriggered`.
#[test]
fn a_profit_stop_confirms_by_time_in_breach_and_is_not_a_limit() -> Result<(), String> {
    let mandate = swing(&[
        (
            "/goal",
            Some(obj(vec![
                ("type", s("profit_stop")),
                ("profit_level", s("0.1")),
                ("end_date", Value::Null),
            ])),
        ),
        ("/risk/max_daily_loss", Some(s("0.5"))),
        ("/risk/scale_lift_after_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            mark("2026-09-21T14:01:00.000000000Z", "110"),
            mark("2026-09-21T14:01:30.000000000Z", "109.9"),
            mark("2026-09-21T14:01:40.000000000Z", "110.1"),
            mark("2026-09-21T14:02:10.000000000Z", "110.2"),
        ],
    )?;
    for number in 1..=3 {
        assert_eq!(
            pending(step_of(&outcomes, number)?),
            vec![LimitKey::ProfitStop],
            "step {number}: the stop is confirming, and it is what `pending` reports"
        );
    }
    assert_eq!(
        step_of(&outcomes, 2)?.snapshot.drawdown,
        ratio("0.000909090909"),
        "10 / 11000, rounded half to even at 12 places"
    );
    let done = step_of(&outcomes, 4)?;
    assert!(pending(done).is_empty());
    assert_eq!(
        restrictions(&done.snapshot),
        vec![Restriction::GoalComplete]
    );
    assert_eq!(done.snapshot.agent_mode, AgentMode::ExitsOnly);
    assert_eq!(
        trace(&done.journal),
        vec![
            "goal done Some(ProfitStopReached) Some(DiscretionaryExitAllThenRetire) None",
            "mode Normal -> ExitsOnly",
        ],
        "a confirmed profit stop carries a reason and a `then`, and no `on_complete`"
    );
    assert!(
        outcomes
            .iter()
            .flat_map(|o| o.journal.iter())
            .all(|e| !matches!(e, RiskEvent::RiskLimitTriggered { .. })),
        "a profit stop is a goal, not a limit"
    );
    Ok(())
}

/// `on_complete: disarm_ladder` stops the ladder and the daily loss and leaves the floor armed (§3.1,
/// planted bug 30).
///
/// C = 10000, so the floor is at C × (1 − 0.1) = 9000. A bid of 93.4 puts E at 9340: a drawdown of 0.066,
/// past the 0.06 rung, and a daily loss of 6.6%, more than three times `max_daily_loss` — both of which
/// would latch here with `breach_confirm_s` of 0, and neither of which may, because the goal disarmed them.
/// A bid of 89.5 then reaches the floor, which `disarm_ladder` does not touch.
#[test]
fn a_disarmed_ladder_stops_the_rungs_and_the_daily_loss_while_the_floor_stays_armed()
-> Result<(), String> {
    let mandate = swing(&[
        ("/goal/on_complete", Some(s("disarm_ladder"))),
        ("/risk/breach_confirm_s", Some(common::i(0))),
    ])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[
            Step {
                at: at("2026-09-21T14:01:00.000000000Z"),
                session: MarketSession::Regular,
                input: Input::GoalComplete,
            },
            mark("2026-09-21T14:02:00.000000000Z", "93.4"),
            mark("2026-09-21T14:03:00.000000000Z", "89.5"),
        ],
    )?;
    let completed = step_of(&outcomes, 1)?;
    assert_eq!(
        trace(&completed.journal),
        vec![
            "goal done None None Some(DisarmLadder)",
            "mode Normal -> ExitsOnly",
        ]
    );
    assert_eq!(
        restrictions(&completed.snapshot),
        vec![Restriction::GoalComplete]
    );

    let disarmed = step_of(&outcomes, 2)?;
    assert_eq!(disarmed.snapshot.drawdown, ratio("0.066"));
    assert_eq!(disarmed.snapshot.daily_pnl_fraction, ratio("-0.066"));
    assert!(
        latched(&disarmed.snapshot).is_empty(),
        "a drawdown past the exits_only rung and a daily loss past its limit both latch nothing"
    );
    assert_eq!(
        disarmed.snapshot.size_factor,
        ratio("1"),
        "and no scale rung arms either"
    );
    assert!(
        trace(&disarmed.journal).is_empty(),
        "a disarmed ladder journals nothing, so nothing has to be lifted later"
    );

    let floored = step_of(&outcomes, 3)?;
    assert_eq!(floored.snapshot.agent_equity, usd("8950"));
    assert_eq!(
        restrictions(&floored.snapshot),
        vec![Restriction::LifetimeFloor, Restriction::GoalComplete],
        "the floor is not disarmed by any `on_complete`"
    );
    assert_eq!(floored.snapshot.agent_mode, AgentMode::Paused);
    assert_eq!(
        trace(&floored.journal),
        vec![
            "triggered LifetimeFloor FlattenAndPause",
            "kill switch Agent for LifetimeFloor",
            "mode ExitsOnly -> Paused",
        ]
    );
    Ok(())
}

/// The journal follows §5.2's evaluation order: the ladder's rungs in ascending `at`, then the daily loss,
/// then the lifetime floor, with each flatten's kill switch beside its own trigger and the mode last.
///
/// One bid of 90 crosses every rung and the floor at once, which is the only way to see the order.
#[test]
fn the_journal_follows_the_evaluation_order() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let clock = RegularSessionClock::new();
    let outcomes = walk(
        &mandate,
        &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
        &clock,
        &[mark("2026-09-21T14:01:00.000000000Z", "90")],
    )?;
    let everything = step_of(&outcomes, 1)?;
    assert_eq!(
        trace(&everything.journal),
        vec![
            "triggered DrawdownRung(0) ScaleSizes",
            "triggered DrawdownRung(1) ExitsOnly",
            "triggered DrawdownRung(2) FlattenAndPause",
            "kill switch Agent for DrawdownRung(2)",
            "triggered LifetimeFloor FlattenAndPause",
            "kill switch Agent for LifetimeFloor",
            "mode Normal -> Paused",
        ]
    );
    assert_eq!(everything.snapshot.drawdown, ratio("0.1"));
    assert_eq!(
        latched(&everything.snapshot),
        vec![
            LimitKey::DrawdownRung(1),
            LimitKey::DrawdownRung(2),
            LimitKey::LifetimeFloor
        ]
    );
    assert_eq!(everything.snapshot.agent_mode, AgentMode::Paused);
    Ok(())
}

/// Crypto counts every second, for equity as well as for the clocks (§5.2).
///
/// 0.1 of an instrument bought at 60000 on a 10000 allocation is an equity of 10000, and a bid of 57990
/// puts it at 9799 — a 2.01% daily loss, past the 2% line. Both facts happen at four in the morning UTC,
/// where an equity has no session at all: a bid there would be ignored and 120 s of it would count as
/// zero. For crypto the bid moves E, and the 120 s both confirm the daily loss and age the mark out.
#[test]
fn crypto_counts_every_second_for_equity_for_confirmation_and_for_staleness() -> Result<(), String>
{
    let mandate = accumulator(&[])?;
    let clock = ContinuousClock;
    let outcomes = walk(
        &mandate,
        &Opening {
            asset_class: AssetClass::Crypto,
            ..opening("2026-09-21T04:00:00.000000000Z", "0.1", "60000", "0")
        },
        &clock,
        &[
            quote(
                "2026-09-21T04:01:00.000000000Z",
                "60000",
                MarketSession::Crypto,
                true,
            ),
            quote(
                "2026-09-21T04:02:00.000000000Z",
                "57990",
                MarketSession::Crypto,
                true,
            ),
            tick("2026-09-21T04:04:00.000000000Z", MarketSession::Crypto),
        ],
    )?;
    assert_eq!(step_of(&outcomes, 1)?.snapshot.agent_equity, usd("10000"));
    let breached = step_of(&outcomes, 2)?;
    assert_eq!(
        breached.snapshot.agent_equity,
        usd("9799"),
        "a crypto bid at 04:02 UTC moves E, where an equity's would be ignored"
    );
    assert_eq!(breached.snapshot.daily_pnl_fraction, ratio("-0.0201"));
    assert_eq!(pending(breached), vec![LimitKey::MaxDailyLoss]);
    assert!(instrument_restrictions(&breached.snapshot).is_empty());

    let later = step_of(&outcomes, 3)?;
    assert_eq!(
        trace(&later.journal),
        vec![
            "triggered MaxDailyLoss ExitsOnly",
            "instrument StaleMark true because NoSaneMark",
            "mode Normal -> ExitsOnly",
        ],
        "120 s of wall clock is 120 s of crypto, so it both confirms the breach and ages the mark"
    );
    assert_eq!(restrictions(&later.snapshot), vec![Restriction::DailyLoss]);
    assert_eq!(
        instrument_restrictions(&later.snapshot),
        vec![InstrumentRestriction::StaleMark]
    );
    Ok(())
}

/// A `mandate-spec` failure as a proptest failure rather than a panic, so a pending run reports the rule
/// that is not implemented instead of unwinding inside a helper.
fn ok<T>(result: Result<T, String>) -> Result<T, TestCaseError> {
    result.map_err(TestCaseError::fail)
}

/// `round(numerator ÷ denominator, 12, half_even)` computed in `i128`, which is the oracle for every ratio
/// §5.2 reports. Nothing here calls `mandate-num`: that is the point.
fn half_even_at_twelve(numerator: i128, denominator: i128) -> Ratio {
    let negative = (numerator < 0) != (denominator < 0);
    let (numerator, denominator) = (numerator.abs(), denominator.abs());
    let scaled = numerator * 1_000_000_000_000;
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    let twice = remainder * 2;
    let quotient = if twice > denominator || (twice == denominator && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    };
    let whole = quotient / 1_000_000_000_000;
    let fraction = quotient % 1_000_000_000_000;
    let magnitude = if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:012}")
            .trim_end_matches('0')
            .to_owned()
    };
    let sign = if negative && quotient != 0 { "-" } else { "" };
    ratio(&format!("{sign}{magnitude}"))
}

/// `ceil(numerator ÷ denominator, 12)` in `i128`, which is the rounding §5.1 scales H, E₀, C, and L by —
/// one rounding, upward, so no scaled quantity ever falls below its exact value (MI-2).
fn ceil_at_twelve(numerator: i128, denominator: i128) -> Usd {
    let scaled = numerator * 1_000_000_000_000;
    let quotient = scaled / denominator;
    let quotient = if scaled % denominator == 0 {
        quotient
    } else {
        quotient + 1
    };
    let whole = quotient / 1_000_000_000_000;
    let fraction = quotient % 1_000_000_000_000;
    let text = if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:012}")
            .trim_end_matches('0')
            .to_owned()
    };
    usd(&text)
}

/// The first input at which a limit confirms, derived the other way round from [`Confirmation`]: find the
/// last point at which the condition had been false for a whole window, then add up the intervals since
/// that began in breach (§5.6). The incremental accumulator and this scan share no code.
fn first_confirmation(inputs: &[(bool, u64)], need: u64) -> Option<usize> {
    let mut condition = vec![false];
    let mut gap = vec![0_u64];
    for (breached, seconds) in inputs {
        condition.push(*breached);
        gap.push(*seconds);
    }
    let mut last_reset = 0_usize;
    for i in 1..gap.len() {
        let mut false_run = 0_u64;
        let mut j = i;
        while j >= 1 && !condition[j - 1] {
            false_run += gap[j];
            j -= 1;
        }
        if !condition[i - 1] && false_run >= need {
            last_reset = i;
        }
        let accumulated: u64 = ((last_reset + 1)..=i)
            .filter(|k| condition[k - 1])
            .map(|k| gap[k])
            .sum();
        if condition[i] && accumulated >= need {
            return Some(i);
        }
    }
    None
}

/// Where §5.2 evaluates each limit: the ladder's rungs in ascending `at`, then the daily loss, then the
/// lifetime floor. Written out here so the order the journal is asserted to be in is this file's reading of
/// §5.2 rather than the crate's.
fn evaluation_rank(limit: LimitKey) -> u32 {
    match limit {
        LimitKey::DrawdownRung(index) => u32::from(index),
        LimitKey::MaxDailyLoss => 100,
        LimitKey::LifetimeFloor => 200,
        LimitKey::ProfitStop => 250,
    }
}

/// The mode the §5.9 severity order gives a set of restrictions, written out here rather than read from
/// the crate, so the two can disagree.
///
/// `daily_loss` asks for `exits_only` here because that is what §5.4's `exits_only` action asks for, and
/// every mandate this property generates from carries that action. A `flatten_and_pause` daily loss asks
/// for `paused` until the owner acknowledges, so the restriction's mode is **not** a function of the
/// restriction alone; `a_daily_flatten_acknowledged_after_flat_leaves_exits_only_until_the_next_day`
/// covers that, and Decisions needed 3 is about the accessor that claims otherwise.
fn strictest(restrictions: &BTreeSet<Restriction>) -> AgentMode {
    restrictions
        .iter()
        .map(|restriction| match restriction {
            Restriction::DailyLoss
            | Restriction::DrawdownExitsOnly
            | Restriction::HardBreach
            | Restriction::GoalComplete => AgentMode::ExitsOnly,
            Restriction::DrawdownFlatten | Restriction::LifetimeFloor => AgentMode::Paused,
            Restriction::Retired => AgentMode::Stopped,
        })
        .max()
        .unwrap_or(AgentMode::Normal)
}

/// A walk of regular-session marks, whole dollars apart so the oracle's arithmetic is exact: 100 shares at
/// a bid of `b` is an equity of exactly 100 × b.
fn bid_walk() -> impl Strategy<Value = Vec<(i64, u64)>> {
    prop::collection::vec((60_i64..140, 30_u64..300), 1..10)
}

/// The bid every walk ends on, so that no property can be satisfied by a fold that does nothing.
///
/// H never falls below the 10000 the walk opens at and never rises above 14000 (the generator's ceiling of
/// 140 × 100 shares), so an equity of 9000 is at least 0.08 × H below it — past the `exits_only` rung, the
/// `flatten_and_pause` rung, and the lifetime floor at C × (1 − 0.1) = 9000, all three of which confirm at
/// once because `ladder_only` sets `breach_confirm_s` to 0. Every property below asserts the consequence
/// **from the bids**, never from the snapshot it is checking: review round 1 found seven of them satisfied
/// by a `step` that returned its opening snapshot with an empty journal.
const ANCHOR_BID: i64 = 90;

/// The latches an [`ANCHOR_BID`] step must leave behind.
fn anchor_latches() -> BTreeSet<LimitKey> {
    BTreeSet::from([
        LimitKey::DrawdownRung(1),
        LimitKey::DrawdownRung(2),
        LimitKey::LifetimeFloor,
    ])
}

/// The steps a [`bid_walk`] describes, starting inside the regular session so every gap is session time,
/// and closing on [`ANCHOR_BID`].
fn bid_steps(walk: &[(i64, u64)]) -> Vec<Step> {
    let mut when = at("2026-09-21T14:00:00.000000000Z").secs();
    let mut steps: Vec<Step> = walk
        .iter()
        .map(|(bid, gap)| {
            when += i64::try_from(*gap).unwrap_or(1);
            Step {
                at: UtcNanos::from_parts(when, 0).expect("an instant"),
                session: MarketSession::Regular,
                input: Input::Mark {
                    bid: price(&bid.to_string()),
                    sane: true,
                },
            }
        })
        .collect();
    when += 300;
    steps.push(Step {
        at: UtcNanos::from_parts(when, 0).expect("an instant"),
        session: MarketSession::Regular,
        input: Input::Mark {
            bid: price(&ANCHOR_BID.to_string()),
            sane: true,
        },
    });
    steps
}

/// Every equity a [`bid_steps`] walk passes through, in `i128` and exactly: 100 shares bought at 100 on a
/// 10000 allocation is an equity of 100 × the bid.
fn bid_equities(walk: &[(i64, u64)]) -> Vec<i128> {
    walk.iter()
        .map(|(bid, _)| i128::from(*bid) * 100)
        .chain([i128::from(ANCHOR_BID) * 100])
        .collect()
}

/// Which `scale_sizes` rungs of [`two_scale_rungs`] are active after each step, derived from the bids
/// alone: a rung arms at `at` × H and stays armed until H − E falls below (`at` − `hysteresis`) × H, which
/// `ladder_only`'s `scale_lift_after_s` of 0 makes immediate. Integer cross-multiplication throughout, so
/// the oracle shares no arithmetic with the crate.
fn active_scale_rungs(walk: &[(i64, u64)]) -> Vec<BTreeSet<u8>> {
    let mut high_water: i128 = 10_000;
    let mut active = [false, false];
    let mut out = Vec::new();
    for equity in bid_equities(walk) {
        high_water = high_water.max(equity);
        let below = high_water - equity;
        for (index, hundredths_at) in [(0_usize, 2_i128), (1, 4)] {
            let armed = below * 100 >= hundredths_at * high_water;
            let held = below * 100 >= (hundredths_at - 1) * high_water;
            active[index] = armed || (active[index] && held);
        }
        out.push(
            active
                .iter()
                .enumerate()
                .filter(|(_, on)| **on)
                .map(|(index, _)| u8::try_from(index).unwrap_or(0))
                .collect(),
        );
    }
    out
}

fn quote_at(when: UtcNanos, bid: &str, session: MarketSession, sane: bool) -> Step {
    Step {
        at: when,
        session,
        input: Input::Mark {
            bid: price(bid),
            sane,
        },
    }
}

fn universe_at(when: UtcNanos, change: UniverseChange, reason: RemovalReason) -> Step {
    Step {
        at: when,
        session: MarketSession::Regular,
        input: Input::UniverseChanged {
            instrument: AssetId::parse(ASSET_A).expect("an asset id"),
            change,
            reason,
        },
    }
}

fn tick_utc(when: UtcNanos, session: MarketSession) -> Step {
    Step {
        at: when,
        session,
        input: Input::Clock,
    }
}

/// The loss the connection carries, taken from the walk's `AgentStopped` event.
fn loss_carry(outcomes: &[Outcome]) -> Option<Usd> {
    outcomes
        .iter()
        .flat_map(|outcome| outcome.journal.iter())
        .find_map(|event| match event {
            RiskEvent::AgentStopped { loss_carry_usd, .. } => Some(*loss_carry_usd),
            _ => None,
        })
}
proptest! {
    /// Equity, the high-water mark, and every reported ratio match an accumulator that shares no code with
    /// the crate, and MI-5 holds at every step: H is never below E, and the drawdown is in [0, 1).
    #[test]
    fn the_reported_figures_match_an_independent_accumulator(bids in bid_walk()) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
            &clock,
            &bid_steps(&bids),
        ))?;
        let mut high_water: i128 = 10_000;
        prop_assert_eq!(outcomes.len(), bids.len() + 1, "the anchor step is the last one");
        for (equity, outcome) in bid_equities(&bids).into_iter().zip(&outcomes) {
            high_water = high_water.max(equity);
            prop_assert_eq!(
                outcome.snapshot.agent_equity,
                usd(&equity.to_string()),
                "E is the allocation plus the position's value at the mark less its basis"
            );
            prop_assert_eq!(outcome.snapshot.high_water_mark, usd(&high_water.to_string()));
            prop_assert_eq!(
                outcome.snapshot.drawdown,
                half_even_at_twelve(high_water - equity, high_water)
            );
            prop_assert_eq!(
                outcome.snapshot.daily_pnl,
                usd(&(equity - 10_000).to_string())
            );
            prop_assert_eq!(
                outcome.snapshot.daily_pnl_fraction,
                half_even_at_twelve(equity - 10_000, 10_000)
            );
            prop_assert!(
                outcome.snapshot.high_water_mark >= outcome.snapshot.agent_equity,
                "MI-5: the high-water mark never falls below equity"
            );
            prop_assert!(
                outcome.snapshot.drawdown >= ratio("0") && outcome.snapshot.drawdown < ratio("1"),
                "MI-5: the drawdown is a fraction of a positive high-water mark"
            );
        }
    }

    /// Breach time matches the interval scan, and a brief recovery never restarts it (§5.6, planted bug 1).
    ///
    /// The generated list is extended with four in-breach intervals so the oracle always confirms: a
    /// property that could be satisfied by never triggering would check nothing.
    #[test]
    fn breach_time_matches_an_independent_interval_accumulator(
        generated in prop::collection::vec((any::<bool>(), 1_u64..90), 1..12),
        need in prop::sample::select(vec![0_u32, 30, 60, 300]),
    ) {
        let mut inputs = generated;
        inputs.extend(std::iter::repeat_n((true, 120), 6));
        let expected = first_confirmation(&inputs, u64::from(need));
        prop_assert!(
            expected.is_some(),
            "the appended in-breach intervals must reach any window, or the property is vacuous"
        );
        let mut confirmation = Confirmation::default();
        let mut actual = None;
        for (index, (breached, seconds)) in inputs.iter().enumerate() {
            if ok(confirmation
                .update(*breached, *seconds, need)
                .map_err(|e| e.to_string()))?
            {
                actual = Some(index + 1);
                break;
            }
        }
        prop_assert_eq!(
            actual,
            expected,
            "the input at which breach time first reaches {} s, over {:?}",
            need,
            inputs
        );
        prop_assert!(
            confirmation.accumulated_s() >= u64::from(need),
            "the limit triggered, so the accumulated breach time is at least the window"
        );
    }

    /// The effective mode is the strictest active restriction, and `AgentModeApplied` appears exactly on a
    /// change (§5.9, MI-6, planted bug 7).
    ///
    /// The oracle keeps its own mode, moved only by the journal's mode events, and its own severity map.
    #[test]
    fn the_mode_is_the_strictest_restriction_and_events_appear_exactly_on_a_change(
        bids in bid_walk()
    ) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
            &clock,
            &bid_steps(&bids),
        ))?;
        let mut from_events = AgentMode::Normal;
        for (number, outcome) in outcomes.iter().enumerate() {
            let mut applied = 0;
            for event in &outcome.journal {
                if let RiskEvent::AgentModeApplied { from, to } = event {
                    prop_assert_eq!(from, &from_events, "step {}: the event's `from` is the mode before it", number + 1);
                    prop_assert_ne!(from, to, "step {}: a mode event that changes nothing", number + 1);
                    from_events = *to;
                    applied += 1;
                }
            }
            prop_assert!(applied <= 1, "step {}: one mode event at most", number + 1);
            prop_assert_eq!(
                outcome.snapshot.agent_mode,
                strictest(&outcome.snapshot.restrictions),
                "step {}: the mode is the strictest restriction", number + 1
            );
            prop_assert_eq!(
                outcome.snapshot.agent_mode,
                from_events,
                "step {}: the journal alone accounts for the mode", number + 1
            );
        }
        prop_assert_eq!(
            outcomes.last().map(|o| o.snapshot.agent_mode),
            Some(AgentMode::Paused),
            "the anchor bid latches a flatten rung and the floor, so the walk cannot end normal"
        );
        prop_assert!(
            outcomes
                .iter()
                .flat_map(|o| o.journal.iter())
                .any(|e| matches!(e, RiskEvent::AgentModeApplied { .. })),
            "and the mode moved at least once, so the journal is not empty"
        );
    }

    /// The latched set and the high-water mark are exactly what the journal says they are (oracle 1), and a
    /// latch never lifts without an acknowledgment, a new risk day, or a loosening version (MI-3).
    #[test]
    fn the_journal_accounts_for_every_latch_and_for_the_high_water_mark(bids in bid_walk()) {
        let mandate = ok(ladder_only(&[]))?;
        let scale_rungs: BTreeSet<u8> = mandate
            .mandate()
            .risk
            .drawdown_ladder
            .iter()
            .enumerate()
            .filter(|(_, rung)| rung.factor.is_some())
            .map(|(index, _)| u8::try_from(index).unwrap_or(u8::MAX))
            .collect();
        let clock = RegularSessionClock::new();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
            &clock,
            &bid_steps(&bids),
        ))?;
        let mut from_events: BTreeSet<LimitKey> = BTreeSet::new();
        let mut high_water = usd("10000");
        for (number, outcome) in outcomes.iter().enumerate() {
            let before = from_events.clone();
            for event in &outcome.journal {
                match event {
                    RiskEvent::RiskLimitTriggered { limit, reason, .. } => {
                        let scale = matches!(limit, LimitKey::DrawdownRung(i) if scale_rungs.contains(i));
                        let pending_only = matches!(reason, Some(TriggerReason::HardBreachPending));
                        if !scale && !pending_only {
                            from_events.insert(*limit);
                        }
                    }
                    RiskEvent::RiskLimitLifted { limit, .. } => {
                        from_events.remove(limit);
                    }
                    RiskEvent::HighWaterMarkReset { to, .. } => high_water = *to,
                    _ => {}
                }
            }
            high_water = high_water.max(outcome.snapshot.agent_equity);
            prop_assert_eq!(
                &outcome.snapshot.latched,
                &from_events,
                "step {}: a limit that latches without journalling, or journals without latching", number + 1
            );
            prop_assert_eq!(
                outcome.snapshot.high_water_mark,
                high_water,
                "step {}: the high-water mark is the running maximum the journal accounts for", number + 1
            );
            prop_assert!(
                before.is_subset(&from_events),
                "step {}: MI-3, no latch lifts on a mark alone", number + 1
            );
        }
        prop_assert_eq!(
            from_events,
            anchor_latches(),
            "the anchor bid latches both stricter rungs and the floor, and the journal says so"
        );
    }

    /// The size factor is the product of the active rungs' factors, is never above one, and is never zero
    /// (§5.5). The oracle multiplies the ladder's own factor texts in `i128`.
    #[test]
    fn the_size_factor_is_the_product_of_the_active_rungs_and_never_exceeds_one(
        bids in bid_walk()
    ) {
        let mandate = ok(ladder_only(&[("/risk/drawdown_ladder", Some(two_scale_rungs()))]))?;
        let clock = RegularSessionClock::new();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
            &clock,
            &bid_steps(&bids),
        ))?;
        let expected = active_scale_rungs(&bids);
        for (number, outcome) in outcomes.iter().enumerate() {
            let armed = expected.get(number).cloned().unwrap_or_default();
            prop_assert_eq!(
                outcome.snapshot.active_rungs.keys().copied().collect::<BTreeSet<u8>>(),
                armed.clone(),
                "step {}: which rungs are armed follows from the bids, not from the snapshot", number + 1
            );
            let mut numerator: i128 = 1;
            let mut denominator: i128 = 1;
            for index in &armed {
                match index {
                    0 => { numerator *= 75; denominator *= 100; }
                    1 => { numerator *= 5; denominator *= 10; }
                    other => prop_assert!(false, "rung {} has no factor and cannot be active", other),
                }
            }
            prop_assert_eq!(
                outcome.snapshot.size_factor,
                half_even_at_twelve(numerator, denominator),
                "step {}: the product of the active rungs' factors", number + 1
            );
            prop_assert!(
                outcome.snapshot.size_factor <= ratio("1") && outcome.snapshot.size_factor > ratio("0"),
                "step {}: a size factor above one would enlarge orders, and zero would stop them", number + 1
            );
        }
        prop_assert_eq!(
            outcomes.last().map(|o| o.snapshot.size_factor),
            Some(ratio("0.375")),
            "the anchor bid is past both scale rungs, so the walk ends at 0.75 x 0.5"
        );
    }

    /// A mark that is not a regular-session sane quote never moves an equity's E, so it can never latch a
    /// limit; the most it does is restrict its own instrument (§5.2, MI-1).
    #[test]
    fn a_mark_outside_the_regular_session_or_off_its_checks_never_latches_a_limit(
        marks in prop::collection::vec((1_i64..400, any::<bool>(), 30_u64..300), 1..10)
    ) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let mut when = at("2026-09-21T12:00:00.000000000Z").secs();
        let mut steps: Vec<Step> = marks
            .iter()
            .map(|(bid, extended, gap)| {
                when += i64::try_from(*gap).unwrap_or(1);
                let (session, sane) = if *extended {
                    (MarketSession::PreMarket, true)
                } else {
                    (MarketSession::Regular, false)
                };
                Step {
                    at: UtcNanos::from_parts(when, 0).expect("an instant"),
                    session,
                    input: Input::Mark { bid: price(&bid.to_string()), sane },
                }
            })
            .collect();
        when += 30;
        steps.push(quote_at(
            UtcNanos::from_parts(when, 0).expect("an instant"),
            "100",
            MarketSession::Regular,
            false,
        ));
        let outcomes = ok(walk(
            &mandate,
            &opening("2026-09-21T11:59:00.000000000Z", "100", "100", "0"),
            &clock,
            &steps,
        ))?;
        for (number, outcome) in outcomes.iter().enumerate() {
            prop_assert_eq!(
                outcome.snapshot.agent_equity,
                usd("10000"),
                "step {}: an ignored or failed mark leaves equity where it was", number + 1
            );
            prop_assert!(
                outcome.snapshot.latched.is_empty(),
                "step {}: and so latches nothing", number + 1
            );
            prop_assert!(
                outcome.snapshot.restrictions.is_empty(),
                "step {}: and restricts the agent not at all", number + 1
            );
            prop_assert_eq!(
                outcome.snapshot.agent_mode,
                AgentMode::Normal,
                "step {}: MI-1", number + 1
            );
            prop_assert!(
                outcome
                    .snapshot
                    .instrument_restrictions
                    .iter()
                    .all(|r| *r == InstrumentRestriction::StaleMark),
                "step {}: the only restriction a mark can raise is its own instrument's", number + 1
            );
        }
        prop_assert_eq!(
            outcomes.last().map(|o| o.snapshot.instrument_restrictions.clone()),
            Some(BTreeSet::from([InstrumentRestriction::StaleMark])),
            "the walk closes on a regular-session mark that failed its checks while the position is held, \
             which §5.2 makes a stale mark whatever the timer says"
        );
    }

    /// Dropping a clock tick that emitted no events changes nothing later (MI-13).
    ///
    /// The augmented walk inserts a tick one second before every mark. Ticks that did emit something are
    /// not silent and the premise does not apply to them, so those cases are discarded rather than
    /// asserted — which is why the assertion is on the final snapshot and not on the journals.
    #[test]
    fn dropping_a_tick_that_emitted_nothing_changes_no_later_result(bids in bid_walk()) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let open = patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0");
        let plain = bid_steps(&bids);
        let mut augmented = Vec::new();
        let mut inserted = Vec::new();
        for step in &plain {
            inserted.push(augmented.len());
            augmented.push(tick_utc(
                UtcNanos::from_parts(step.at.secs() - 1, 0).expect("an instant"),
                MarketSession::Regular,
            ));
            augmented.push(step.clone());
        }
        let without = ok(walk(&mandate, &open, &clock, &plain))?;
        let with = ok(walk(&mandate, &open, &clock, &augmented))?;
        prop_assume!(
            inserted
                .iter()
                .all(|index| with.get(*index).is_some_and(|o| o.journal.is_empty()))
        );
        let expected = usd(&(i128::from(ANCHOR_BID) * 100).to_string());
        prop_assert_eq!(
            without.last().map(|o| o.snapshot.agent_equity),
            Some(expected),
            "the walk closes on the anchor bid, so the fold must have moved at all"
        );
        prop_assert_eq!(
            without.last().map(|o| o.snapshot.latched.clone()),
            Some(anchor_latches())
        );
        prop_assert_eq!(
            without.last().map(|o| o.snapshot.clone()),
            with.last().map(|o| o.snapshot.clone()),
            "the silent ticks changed the state they were dropped from"
        );
    }

    /// The same mandate and the same inputs give the same outputs (MI-8, ES-21).
    #[test]
    fn identical_inputs_give_identical_states_and_events(bids in bid_walk()) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let open = patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0");
        let steps = bid_steps(&bids);
        let once = ok(walk(&mandate, &open, &clock, &steps))?;
        let twice = ok(walk(&mandate, &open, &clock, &steps))?;
        prop_assert_eq!(
            once.last().map(|o| o.snapshot.agent_equity),
            Some(usd(&(i128::from(ANCHOR_BID) * 100).to_string())),
            "the walk closes on the anchor bid, so two identical runs are not two empty ones"
        );
        prop_assert_eq!(
            once.last().map(|o| o.snapshot.latched.clone()),
            Some(anchor_latches())
        );
        prop_assert_eq!(once, twice, "two runs of one input list disagreed");
    }

    /// An applied allocation change preserves every ratio it reports and arms nothing; a refused one
    /// changes nothing at all (§5.1, MI-2, MI-7, planted bugs 4 and 5).
    #[test]
    fn an_applied_allocation_change_preserves_every_ratio_and_arms_nothing(
        bid in 60_i64..140,
        delta in -9_000_i64..9_000,
    ) {
        prop_assume!(delta != 0);
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "500"),
            &clock,
            &[
                mark("2026-09-21T14:01:00.000000000Z", &bid.to_string()),
                allocate("2026-09-21T14:02:00.000000000Z", &delta.to_string()),
            ],
        ))?;
        let before = &ok(step_of(&outcomes, 1))?.snapshot;
        let change = ok(step_of(&outcomes, 2))?;
        let after = &change.snapshot;
        if change.rejection.is_some() {
            prop_assert_eq!(after.agent_equity, before.agent_equity);
            prop_assert_eq!(after.high_water_mark, before.high_water_mark);
            prop_assert_eq!(after.drawdown, before.drawdown);
            prop_assert_eq!(after.day_start_equity, before.day_start_equity);
            prop_assert_eq!(after.capital_base, before.capital_base);
            prop_assert_eq!(after.net_contributed, before.net_contributed);
            prop_assert_eq!(&after.latched, &before.latched);
            prop_assert_eq!(&after.restrictions, &before.restrictions);
            prop_assert_eq!(
                after.inherited_loss,
                before.inherited_loss,
                "a refused change scales the inherited loss no more than anything else"
            );
            return Ok(());
        }
        prop_assert!(
            after.drawdown >= before.drawdown,
            "MI-2: scaling rounds up, so the drawdown never falls ({} -> {})",
            before.drawdown,
            after.drawdown
        );
        prop_assert!(
            after.daily_pnl_fraction <= before.daily_pnl_fraction,
            "MI-2: nor does the daily loss fraction rise ({} -> {})",
            before.daily_pnl_fraction,
            after.daily_pnl_fraction
        );
        prop_assert_eq!(
            &after.latched,
            &before.latched,
            "MI-2: an applied change neither triggers nor lifts a limit"
        );
        prop_assert_eq!(&after.restrictions, &before.restrictions);
        prop_assert_eq!(after.size_factor, before.size_factor);
        prop_assert!(
            change.journal.iter().all(|event| !matches!(
                event,
                RiskEvent::RiskLimitTriggered { .. }
                    | RiskEvent::RiskLimitLifted { .. }
                    | RiskEvent::KillSwitchActivated { .. }
            )),
            "and journals no limit change either"
        );
        prop_assert!(
            after.net_contributed != before.net_contributed,
            "an applied change moves net contributed, which the loss carry is measured from"
        );
        let equity = i128::from(bid) * 100;
        prop_assert_eq!(
            before.inherited_loss,
            usd("500"),
            "the agent opened with a connection loss carry, which §5.1 scales like C"
        );
        prop_assert_eq!(
            after.inherited_loss,
            ceil_at_twelve(500 * (equity + i128::from(delta)), equity),
            "L' is ceil(L x (E + delta) / E, 12), which a fold that dropped L would report as zero"
        );
    }

    /// The loss carried to the connection is the same however the allocation moved first (MI-14, planted
    /// bug 6): it is max(0, net contributed − E), and a withdrawal lowers both by the same dollars.
    #[test]
    fn the_loss_carry_is_invariant_under_a_withdraw_then_deposit_pair(
        bid in 98_i64..=100,
        withdrawal in 1_i64..5_000,
    ) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let open = patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0");
        let flat = [
            mark("2026-09-21T14:01:00.000000000Z", &bid.to_string()),
            sale(
                "2026-09-21T14:02:00.000000000Z",
                "100",
                &bid.to_string(),
                MarketSession::Regular,
            ),
        ];
        let down = allocate("2026-09-21T14:03:00.000000000Z", &format!("-{withdrawal}"));
        let up = allocate("2026-09-21T14:04:00.000000000Z", &withdrawal.to_string());
        let straight: Vec<Step> = flat
            .iter()
            .cloned()
            .chain([retire("2026-09-21T14:05:00.000000000Z")])
            .collect();
        let withdrawn: Vec<Step> = flat
            .iter()
            .cloned()
            .chain([down.clone(), retire("2026-09-21T14:05:00.000000000Z")])
            .collect();
        let round_trip: Vec<Step> = flat
            .iter()
            .cloned()
            .chain([down, up, retire("2026-09-21T14:05:00.000000000Z")])
            .collect();
        let expected = usd(&(10_000 - bid * 100).to_string());
        for (name, steps) in [
            ("no allocation change", straight),
            ("a withdrawal", withdrawn),
            ("a withdrawal and a deposit", round_trip),
        ] {
            let outcomes = ok(walk(&mandate, &open, &clock, &steps))?;
            prop_assert_eq!(
                loss_carry(&outcomes),
                Some(expected),
                "{}: the carry is max(0, net contributed - E) whatever came first",
                name
            );
        }
    }

    /// A completed goal never adds risk, whichever `on_complete` the owner chose (§3.1).
    ///
    /// The mode may only tighten and no limit may lift: `hold_protected` holds everything armed,
    /// `disarm_ladder` disarms the ladder and the daily loss, and `release` retires the agent — and none of
    /// the three is a path by which a latched limit goes away.
    #[test]
    fn a_completed_goal_never_adds_risk(
        on_complete in prop::sample::select(vec!["hold_protected", "disarm_ladder", "release"]),
        bid in 60_i64..140,
    ) {
        let mandate = ok(ladder_only(&[("/goal/on_complete", Some(s(on_complete)))]))?;
        let clock = RegularSessionClock::new();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
            &clock,
            &[
                mark("2026-09-21T14:01:00.000000000Z", &bid.to_string()),
                Step {
                    at: at("2026-09-21T14:02:00.000000000Z"),
                    session: MarketSession::Regular,
                    input: Input::GoalComplete,
                },
            ],
        ))?;
        let before = &ok(step_of(&outcomes, 1))?.snapshot;
        let completed = ok(step_of(&outcomes, 2))?;
        prop_assert!(
            completed.snapshot.agent_mode >= before.agent_mode,
            "`{}` loosened the mode from {:?} to {:?}",
            on_complete,
            before.agent_mode,
            completed.snapshot.agent_mode
        );
        prop_assert!(
            before.latched.is_subset(&completed.snapshot.latched),
            "`{}` lifted a latch that completing a goal is not a path out of",
            on_complete
        );
        prop_assert!(
            completed.journal.iter().all(|event| !matches!(
                event,
                RiskEvent::RiskLimitLifted { .. }
            )),
            "`{}` journalled a lift: {:?}",
            on_complete,
            completed.journal
        );
        prop_assert!(
            completed.snapshot.size_factor <= before.size_factor,
            "`{}` raised the size factor",
            on_complete
        );
        let expected = match on_complete {
            "release" => Restriction::Retired,
            _ => Restriction::GoalComplete,
        };
        prop_assert!(
            completed.snapshot.restrictions.contains(&expected),
            "`{}` must leave {:?} behind, and left {:?}",
            on_complete,
            expected,
            completed.snapshot.restrictions
        );
        let applied = match on_complete {
            "hold_protected" => "HoldProtected",
            "disarm_ladder" => "DisarmLadder",
            _ => "Release",
        };
        prop_assert_eq!(
            trace(&completed.journal).first().cloned(),
            Some(format!("goal done None None Some({applied})")),
            "`{}`: the completion is journalled first, with the `on_complete` the owner chose",
            on_complete
        );
        if on_complete == "release" {
            prop_assert_eq!(
                completed.snapshot.agent_mode,
                AgentMode::Stopped,
                "`release` retires the agent"
            );
            prop_assert!(
                completed
                    .journal
                    .iter()
                    .any(|event| matches!(event, RiskEvent::PositionReleased { .. })),
                "and hands the position to the owner"
            );
        }
    }

    /// One bad print never latches a limit or flattens anything (§5.6, MI-4, DEC-63, planted bug 2).
    ///
    /// A healthy mark, then one wild print at any depth, then a sane quote back above it less than the hard
    /// wait later. Soft confirmation needs the whole 60 s window and the hard path needs a second quote at
    /// min(`breach_confirm_s`, 10) s, so neither can have completed: whatever the print said, nothing may be
    /// latched and no kill switch may have fired.
    #[test]
    fn one_bad_print_never_latches_a_limit(wild in 1_i64..96, gap_s in 1_i64..10) {
        let mandate = ok(swing(&[("/risk/max_daily_loss", Some(s("0.5")))]))?;
        let clock = RegularSessionClock::new();
        let base = at("2026-09-21T14:01:00.000000000Z").secs();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
            &clock,
            &[
                mark("2026-09-21T14:01:00.000000000Z", "105"),
                quote_at(
                    UtcNanos::from_parts(base + 60, 0).expect("an instant"),
                    &wild.to_string(),
                    MarketSession::Regular,
                    true,
                ),
                quote_at(
                    UtcNanos::from_parts(base + 60 + gap_s, 0).expect("an instant"),
                    "130",
                    MarketSession::Regular,
                    true,
                ),
            ],
        ))?;
        for (number, outcome) in outcomes.iter().enumerate() {
            prop_assert!(
                outcome.snapshot.latched.is_empty(),
                "step {}: a bid of {} for {} s latched {:?}",
                number + 1,
                wild,
                gap_s,
                outcome.snapshot.latched
            );
            prop_assert!(
                outcome.journal.iter().all(|event| !matches!(
                    event,
                    RiskEvent::KillSwitchActivated { .. }
                )),
                "step {}: a bid of {} for {} s flattened the agent",
                number + 1,
                wild,
                gap_s
            );
        }
        let printed = ok(step_of(&outcomes, 2))?;
        prop_assert_eq!(
            printed.snapshot.agent_equity,
            usd(&(i128::from(wild) * 100).to_string()),
            "the print moved equity, so the fold did something with it"
        );
        prop_assert_eq!(
            printed.snapshot.restrictions.clone(),
            BTreeSet::from([Restriction::HardBreach]),
            "every bid under 96 is past the 1.25 x 0.06 x 10500 = 787.5 hard level, which §5.6 escalates \
             to `exits_only` at once and latches nothing"
        );
        prop_assert_eq!(printed.snapshot.agent_mode, AgentMode::ExitsOnly);
        let recovered = ok(step_of(&outcomes, 3))?;
        prop_assert_eq!(recovered.snapshot.agent_equity, usd("13000"));
        prop_assert!(
            recovered.snapshot.restrictions.is_empty(),
            "and the quote above the level clears it"
        );
        prop_assert_eq!(
            recovered.snapshot.agent_mode,
            AgentMode::Normal,
            "the print's temporary `exits_only` is cleared by the quote above the level"
        );
    }

    /// Every step's journal is in §5.2's evaluation order: the rungs in ascending `at`, then the daily loss,
    /// then the lifetime floor, each flatten's kill switch beside its own trigger, and the mode last.
    #[test]
    fn emitted_events_are_in_the_spec_order(bids in bid_walk()) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T14:00:00.000000000Z", "100", "100", "0"),
            &clock,
            &bid_steps(&bids),
        ))?;
        let mut ordered = 0;
        for (number, outcome) in outcomes.iter().enumerate() {
            let mut previous = 0_u32;
            let mut last_limit: Option<LimitKey> = None;
            for event in &outcome.journal {
                let rank = match event {
                    RiskEvent::RiskLimitTriggered { limit, .. }
                    | RiskEvent::RiskLimitLifted { limit, .. } => evaluation_rank(*limit),
                    RiskEvent::KillSwitchActivated { initiator, .. } => {
                        prop_assert_eq!(
                            last_limit,
                            Some(*initiator),
                            "step {}: a kill switch must follow its own trigger", number + 1
                        );
                        evaluation_rank(*initiator)
                    }
                    RiskEvent::InstrumentRestrictionChanged { .. } => 300,
                    RiskEvent::AgentModeApplied { .. } => 400,
                    _ => previous,
                };
                prop_assert!(
                    rank >= previous,
                    "step {}: {:?} came after rank {}, which §5.2 orders the other way",
                    number + 1,
                    event,
                    previous
                );
                previous = rank;
                last_limit = match event {
                    RiskEvent::RiskLimitTriggered { limit, .. } => Some(*limit),
                    _ => last_limit,
                };
                ordered += 1;
            }
        }
        prop_assert!(
            ordered >= 4,
            "the anchor bid latches two rungs and the floor with their kill switches, so there is an order \
             to be in: only {ordered} events were journalled"
        );
    }

    /// One `InstrumentRestrictionChanged` per restriction that changed, never one standing for another
    /// (§5.9, §5.10).
    #[test]
    fn one_instrument_event_per_changed_restriction(
        changes in prop::collection::vec((any::<bool>(), 30_u64..300), 1..10)
    ) {
        let mandate = ok(ladder_only(&[]))?;
        let clock = RegularSessionClock::new();
        let mut when = at("2026-09-21T14:00:00.000000000Z").secs();
        let mut steps: Vec<Step> = changes
            .iter()
            .map(|(removed, gap)| {
                when += i64::try_from(*gap).unwrap_or(1);
                let (change, reason) = if *removed {
                    (UniverseChange::Removed, RemovalReason::ThesisExpired)
                } else {
                    (UniverseChange::Admitted, RemovalReason::ThesisAdmitted)
                };
                Step {
                    at: UtcNanos::from_parts(when, 0).expect("an instant"),
                    session: MarketSession::Regular,
                    input: Input::UniverseChanged {
                        instrument: AssetId::parse(ASSET_A).expect("an asset id"),
                        change,
                        reason,
                    },
                }
            })
            .collect();
        when += 30;
        steps.push(universe_at(
            UtcNanos::from_parts(when, 0).expect("an instant"),
            UniverseChange::Removed,
            RemovalReason::ThesisExpired,
        ));
        let outcomes = ok(walk(
            &mandate,
            &patient_opening("2026-09-21T13:59:00.000000000Z", "100", "100", "0"),
            &clock,
            &steps,
        ))?;
        let mut held: BTreeSet<InstrumentRestriction> = BTreeSet::new();
        for (number, outcome) in outcomes.iter().enumerate() {
            let events: Vec<(InstrumentRestriction, bool)> = outcome
                .journal
                .iter()
                .filter_map(|event| match event {
                    RiskEvent::InstrumentRestrictionChanged { restriction, active, .. } => {
                        Some((*restriction, *active))
                    }
                    _ => None,
                })
                .collect();
            let changed: BTreeSet<InstrumentRestriction> = held
                .symmetric_difference(&outcome.snapshot.instrument_restrictions)
                .copied()
                .collect();
            prop_assert_eq!(
                events.len(),
                changed.len(),
                "step {}: one event per restriction that changed, and none for one that did not",
                number + 1
            );
            for (restriction, active) in &events {
                prop_assert!(
                    changed.contains(restriction),
                    "step {}: an event for a restriction that did not change", number + 1
                );
                prop_assert_eq!(
                    *active,
                    outcome.snapshot.instrument_restrictions.contains(restriction),
                    "step {}: the event's `active` is the state it left behind", number + 1
                );
            }
            held = outcome.snapshot.instrument_restrictions.clone();
        }
        prop_assert_eq!(
            held,
            BTreeSet::from([InstrumentRestriction::RemovedInstrument]),
            "the walk closes on a removal, so the instrument is restricted and nothing else is"
        );
    }
}
