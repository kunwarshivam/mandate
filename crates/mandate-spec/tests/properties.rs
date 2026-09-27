//! Property tests for the risk state, risk days, and goals (mandate spec §5, §5.4, §3.1), story E6-4.
//!
//! Each property here has an oracle that computes its answer a second way, from something the rule
//! under test does not share: the journal a step emitted, an accumulator over the input list, the
//! calendar, or a second run of the same market. A property whose oracle called the implementation's
//! own predicate would pass on the bug it is named after (AGENTS.md, "Independent oracles").
//!
//! The families S, V, P, and C have their own invariants; they belong to the PRs that carry those
//! families.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{NyseRegularSeconds, fraction, i, instant, ladder_rung, s, validated, with_all};
use mandate_canon::Value;
use mandate_domain::{AgentMode, AssetClass, MarketSession};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_spec::document::{LadderAction, LadderRung};
use mandate_spec::risk::{
    Confirmation, Input, LimitKey, Opening, Outcome, Restriction, RiskEvent, RiskState, Step,
    StopReason, risk_day, size_factor,
};
use mandate_time::{Date, UtcNanos, new_york_midnight};
use proptest::prelude::*;

const START: &str = "2026-09-21T14:00:00.000000000Z";
/// One minute between marks, so a twelve-step run stays inside one regular session.
const STEP_S: i64 = 60;

fn document(changes: &[(&str, Option<Value>)]) -> Value {
    let mut all: Vec<(&str, Option<Value>)> = vec![
        ("/risk/max_position_usd", Some(s("10000"))),
        ("/risk/max_position_fraction", Some(s("1"))),
        ("/risk/max_gross_exposure_usd", Some(s("10000"))),
        ("/risk/max_daily_loss", Some(s("0.05"))),
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

fn open(mandate: &Value) -> RiskState {
    RiskState::open(&validated(mandate), &opening(), &NyseRegularSeconds).expect("the state opens")
}

/// A generated amount in hundredths, as the canonical decimal text every grammar in the workspace
/// wants: no trailing zero in the fraction, and no fraction at all when there is none. A float never
/// appears, and `Price::parse` accepts the result unchanged.
fn hundredths_text(value: u32) -> String {
    let (whole, rest) = (value / 100, value % 100);
    match (rest % 10, rest) {
        (_, 0) => format!("{whole}"),
        (0, _) => format!("{whole}.{}", rest / 10),
        _ => format!("{whole}.{rest:02}"),
    }
}

fn at_second(offset: i64) -> UtcNanos {
    UtcNanos::from_parts(instant(START).secs() + offset, 0).expect("an instant")
}

/// A generated market: one sane regular-session mark a minute, each bid between $80 and $120, so
/// equity walks between $8 000 and $12 000 across every rung of the base ladder.
fn marks() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::vec(8000_u32..=12000, 1..12)
}

fn mark_steps(bids: &[u32]) -> Vec<Step> {
    bids.iter()
        .enumerate()
        .map(|(index, cents)| Step {
            at: at_second(STEP_S * (i64::try_from(index).unwrap_or_default() + 1)),
            session: MarketSession::Regular,
            input: Input::Mark {
                bid: Price::parse(&hundredths_text(*cents)).expect("a price"),
                sane: true,
            },
        })
        .collect()
}

fn run(mandate: &Value, steps: &[Step]) -> Vec<Outcome> {
    let mut state = open(mandate);
    steps
        .iter()
        .map(|step| state.step(step).expect("an evaluable step"))
        .collect()
}

/// Oracle 1: the latched set, the restrictions, the mode, and the high-water mark rebuilt **only**
/// from the events the steps emitted. A limit that latches without journalling, or journals without
/// latching, makes this disagree with the snapshot.
#[derive(Default)]
struct FromTheJournal {
    latched: BTreeSet<LimitKey>,
    restrictions: BTreeSet<Restriction>,
    mode: Option<AgentMode>,
    high_water_mark: Option<Usd>,
}

impl FromTheJournal {
    fn absorb(&mut self, events: &[RiskEvent]) {
        for event in events {
            match event {
                RiskEvent::RiskLimitTriggered { limit, .. } => {
                    self.latched.insert(*limit);
                }
                RiskEvent::RiskLimitLifted { limit, .. } => {
                    self.latched.remove(limit);
                }
                RiskEvent::AgentModeApplied { to, .. } => self.mode = Some(*to),
                RiskEvent::HighWaterMarkReset { to, .. } => self.high_water_mark = Some(*to),
                RiskEvent::AgentStopped { .. } => {
                    self.restrictions.insert(Restriction::Retired);
                }
                _ => {}
            }
        }
    }
}

/// Oracle 2: breach time accumulated from the input list alone, crediting each interval by the
/// condition at its **start** and resetting only after a full window of falsity. It shares no code
/// with [`Confirmation`], which is what makes it able to catch planted bug 1.
#[derive(Default)]
struct IntervalAccumulator {
    accumulated_s: u64,
    false_run_s: u64,
    was_breached: bool,
}

impl IntervalAccumulator {
    fn credit(&mut self, breached: bool, elapsed_s: u64, need_s: u32) -> bool {
        if self.was_breached {
            self.accumulated_s = self.accumulated_s.saturating_add(elapsed_s);
            self.false_run_s = 0;
        } else {
            self.false_run_s = self.false_run_s.saturating_add(elapsed_s);
            if self.false_run_s >= u64::from(need_s) {
                self.accumulated_s = 0;
            }
        }
        self.was_breached = breached;
        breached && self.accumulated_s >= u64::from(need_s)
    }
}

proptest! {
    /// §5.6 through an accumulator written from the spec sentence rather than from the type: the two
    /// must agree on when the limit triggers **and** on how much breach time has been credited, so a
    /// reset on the first false interval is caught even when it changes no trigger.
    #[test]
    #[ignore = "pending E6-4"]
    fn confirmation_matches_an_independent_interval_accumulator(
        series in prop::collection::vec((any::<bool>(), 1_u64..90), 1..20),
        need_s in 0_u32..=300,
    ) {
        let mut confirm = Confirmation::default();
        let mut oracle = IntervalAccumulator::default();
        for (breached, elapsed_s) in series {
            let triggered = confirm.update(breached, elapsed_s, need_s);
            let expected = oracle.credit(breached, elapsed_s, need_s);
            prop_assert_eq!(triggered, expected, "the trigger must agree with the accumulator");
            prop_assert_eq!(
                confirm.accumulated_s(),
                oracle.accumulated_s,
                "and so must the breach time itself"
            );
        }
    }

    /// MI-5: H is the maximum equity since the last reset, so H >= E always, and the reported
    /// drawdown is in [0, 1). The oracle is a running maximum over the equity the snapshots report,
    /// which never consults the state's own high-water mark.
    #[test]
    #[ignore = "pending E6-4"]
    fn the_high_water_mark_never_falls_below_equity(bids in marks()) {
        let mandate = document(&[]);
        let steps = mark_steps(&bids);
        let mut highest = Usd::parse("10000").expect("the opening allocation");
        for outcome in run(&mandate, &steps) {
            highest = highest.max(outcome.snapshot.agent_equity);
            prop_assert_eq!(
                outcome.snapshot.high_water_mark,
                highest,
                "H is the running maximum of E"
            );
            prop_assert!(outcome.snapshot.drawdown >= Ratio::ZERO);
            prop_assert!(outcome.snapshot.drawdown < Ratio::parse("1").expect("one"));
        }
    }

    /// MI-6: `AgentModeApplied` appears exactly when the effective mode changes — never on a step that
    /// left it alone, and never missing from one that moved it. The oracle is the previous snapshot's
    /// mode, which the event itself does not carry from.
    #[test]
    #[ignore = "pending E6-4"]
    fn mode_events_appear_exactly_on_a_change(bids in marks()) {
        let mandate = document(&[]);
        let steps = mark_steps(&bids);
        let mut previous = AgentMode::Normal;
        for outcome in run(&mandate, &steps) {
            let events: Vec<(AgentMode, AgentMode)> = outcome
                .journal
                .iter()
                .filter_map(|event| match event {
                    RiskEvent::AgentModeApplied { from, to } => Some((*from, *to)),
                    _ => None,
                })
                .collect();
            let now = outcome.snapshot.agent_mode;
            if now == previous {
                prop_assert!(events.is_empty(), "no change, so no event: {:?}", events);
            } else {
                prop_assert_eq!(events, vec![(previous, now)], "one event for one change");
            }
            previous = now;
        }
    }

    /// §5.2's evaluation order, as a property of every generated market: the rungs a step triggers
    /// come out in ascending index, the daily loss after them, and the mode last.
    #[test]
    #[ignore = "pending E6-4"]
    fn emitted_events_are_in_the_spec_order(bids in marks()) {
        let mandate = document(&[("/risk/breach_confirm_s", Some(i(0)))]);
        let steps = mark_steps(&bids);
        for outcome in run(&mandate, &steps) {
            let mut last_rung: Option<u8> = None;
            let mut seen_daily = false;
            for (index, event) in outcome.journal.iter().enumerate() {
                match event {
                    RiskEvent::RiskLimitTriggered { limit: LimitKey::DrawdownRung(rung), .. } => {
                        prop_assert!(!seen_daily, "a rung after the daily loss");
                        if let Some(previous) = last_rung {
                            prop_assert!(*rung > previous, "rungs in ascending `at`");
                        }
                        last_rung = Some(*rung);
                    }
                    RiskEvent::RiskLimitTriggered { limit: LimitKey::MaxDailyLoss, .. } => {
                        seen_daily = true;
                    }
                    RiskEvent::AgentModeApplied { .. } => {
                        prop_assert_eq!(
                            index + 1,
                            outcome.journal.len(),
                            "the effective mode is the last thing a step decides"
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    /// Oracle 1 as a property: the snapshot's latched set and mode are exactly what the journal says
    /// they are, over every generated market.
    #[test]
    #[ignore = "pending E6-4"]
    fn the_journal_rebuilds_the_latched_set_and_the_mode(bids in marks()) {
        let mandate = document(&[("/risk/breach_confirm_s", Some(i(0)))]);
        let steps = mark_steps(&bids);
        let mut oracle = FromTheJournal::default();
        for outcome in run(&mandate, &steps) {
            oracle.absorb(&outcome.journal);
            prop_assert_eq!(
                outcome.snapshot.latched.clone(),
                oracle.latched.clone(),
                "a limit that latches without journalling, or journals without latching"
            );
            prop_assert_eq!(
                outcome.snapshot.agent_mode,
                oracle.mode.unwrap_or(AgentMode::Normal),
                "the mode the journal implies is the mode the snapshot reports"
            );
        }
    }

    /// MI-8: the same mandate and the same inputs give the same state and the same events. Nothing
    /// here reads a clock or a hash map, so a second run cannot differ (ES-21).
    #[test]
    #[ignore = "pending E6-4"]
    fn identical_inputs_give_identical_states_and_events(bids in marks()) {
        let mandate = document(&[]);
        let steps = mark_steps(&bids);
        prop_assert_eq!(run(&mandate, &steps), run(&mandate, &steps));
    }

    /// MI-13: a clock tick that emitted no events can be dropped without changing any later result,
    /// because conditions change at inputs and not at ticks.
    #[test]
    #[ignore = "pending E6-4"]
    fn silent_ticks_can_be_dropped(bids in marks()) {
        let mandate = document(&[]);
        let steps = mark_steps(&bids);
        let with_ticks: Vec<Step> = steps
            .iter()
            .flat_map(|step| {
                [
                    Step {
                        at: UtcNanos::from_parts(step.at.secs() - 30, 0).expect("an instant"),
                        session: MarketSession::Regular,
                        input: Input::Clock,
                    },
                    step.clone(),
                ]
            })
            .collect();
        let plain = run(&mandate, &steps);
        let ticked = run(&mandate, &with_ticks);
        let kept: Vec<Outcome> = ticked
            .into_iter()
            .enumerate()
            .filter(|(index, _)| index % 2 == 1)
            .map(|(_, outcome)| outcome)
            .collect();
        prop_assert_eq!(kept, plain, "the ticks emitted nothing, so they changed nothing");
    }

    /// §5.5: every rung's factor is at most one, so their product is too — a de-risking factor can
    /// never enlarge an order. The oracle multiplies the generated factors itself.
    #[test]
    #[ignore = "pending E6-4"]
    fn the_size_factor_never_exceeds_one(
        factors in prop::collection::vec(1_u32..=100, 1..5),
    ) {
        let ladder: Vec<LadderRung> = factors
            .iter()
            .enumerate()
            .map(|(index, hundredths)| {
                ladder_rung(
                    &format!("0.0{}", index + 1),
                    LadderAction::ScaleSizes,
                    Some(&hundredths_text(*hundredths)),
                )
            })
            .collect();
        let active: BTreeMap<u8, u64> = (0..ladder.len())
            .map(|index| (u8::try_from(index).unwrap_or_default(), 0))
            .collect();
        let found = size_factor(&ladder, &active).expect("evaluable");
        prop_assert!(found <= Ratio::parse("1").expect("one"));
        prop_assert!(found > Ratio::ZERO, "a factor is strictly positive (open_fraction)");
    }

    /// MI-4, DEC-63: one sane print at or past a hard level applies `hard_breach` and latches nothing,
    /// whatever the price and whatever the limit. The oracle is the latched set itself, which must
    /// still be the one the step before produced.
    #[test]
    #[ignore = "pending E6-4"]
    fn one_bad_print_never_latches_a_limit(cents in 1000_u32..=7000) {
        let mandate = document(&[("/risk/max_daily_loss", Some(s("0.5")))]);
        let steps = vec![
            Step {
                at: at_second(STEP_S),
                session: MarketSession::Regular,
                input: Input::Mark {
                    bid: Price::parse(&hundredths_text(cents)).expect("a price"),
                    sane: true,
                },
            },
        ];
        let outcomes = run(&mandate, &steps);
        let print = outcomes.last().expect("one step");
        prop_assert!(
            print.snapshot.latched.is_empty(),
            "one quote latched {:?}",
            print.snapshot.latched
        );
        prop_assert!(print.snapshot.agent_mode <= AgentMode::ExitsOnly, "a flatten on one print");
    }

    /// §5.2: a mark that is not sane, or an equity mark outside the regular session, moves nothing —
    /// so it can neither latch a limit nor lift one.
    #[test]
    #[ignore = "pending E6-4"]
    fn a_stale_or_insane_mark_never_latches_a_limit(cents in 1000_u32..=20000, sane in any::<bool>()) {
        let mandate = document(&[]);
        let session = if sane { MarketSession::AfterHours } else { MarketSession::Regular };
        let steps = vec![Step {
            at: at_second(STEP_S),
            session,
            input: Input::Mark {
                bid: Price::parse(&hundredths_text(cents)).expect("a price"),
                sane,
            },
        }];
        let outcomes = run(&mandate, &steps);
        let ignored = outcomes.last().expect("one step");
        prop_assert_eq!(
            ignored.snapshot.agent_equity,
            Usd::parse("10000").expect("the opening allocation"),
            "an unusable mark leaves E where it was"
        );
        prop_assert!(ignored.snapshot.latched.is_empty());
    }

    /// MI-2, MI-7: an applied allocation change preserves drawdown and the daily loss fraction (up to
    /// §5.1's conservative rounding up) and arms nothing. The oracle is the snapshot before the
    /// change, so the property never asks the scaling code what it should have produced.
    #[test]
    #[ignore = "pending E6-4"]
    fn an_applied_allocation_change_preserves_every_ratio_and_arms_nothing(
        cents in 9000_u32..=11000,
        delta in -4000_i32..=-100,
    ) {
        let mandate = document(&[]);
        let mut state = open(&mandate);
        let before = state
            .step(&Step {
                at: at_second(STEP_S),
                session: MarketSession::Regular,
                input: Input::Mark {
                    bid: Price::parse(&hundredths_text(cents)).expect("a price"),
                    sane: true,
                },
            })
            .expect("an evaluable mark");
        let flat = state
            .step(&Step {
                at: at_second(STEP_S * 2),
                session: MarketSession::Regular,
                input: Input::Fill {
                    side: mandate_domain::Side::Sell,
                    qty: Qty::parse("100").expect("a quantity"),
                    price: Price::parse(&hundredths_text(cents)).expect("a price"),
                },
            })
            .expect("an evaluable fill");
        prop_assert_eq!(flat.rejection, None);
        let after = state
            .step(&Step {
                at: at_second(STEP_S * 3),
                session: MarketSession::Regular,
                input: Input::AllocationChange {
                    delta_usd: Usd::parse(&format!("{delta}")).expect("a dollar amount"),
                },
            })
            .expect("an evaluable change");
        prop_assume!(after.rejection.is_none());
        prop_assert!(
            after.snapshot.drawdown >= before.snapshot.drawdown,
            "scaling rounds up, so drawdown never falls"
        );
        prop_assert!(
            after.snapshot.daily_pnl_fraction <= before.snapshot.daily_pnl_fraction,
            "and the daily loss fraction never improves"
        );
        prop_assert_eq!(
            after.snapshot.latched,
            before.snapshot.latched,
            "an allocation change neither triggers nor lifts a limit"
        );
        prop_assert_eq!(after.snapshot.restrictions, before.snapshot.restrictions);
    }

    /// MI-14: the loss carry is max(0, net contributed − E), so a withdrawal followed by the matching
    /// deposit leaves it exactly where it was. The oracle is the run without the pair.
    #[test]
    #[ignore = "pending E6-4"]
    fn the_loss_carry_is_invariant_under_a_withdraw_then_deposit_pair(
        cents in 8500_u32..=9500,
        amount in 100_u32..=3000,
    ) {
        let mandate = document(&[("/risk/max_daily_loss", Some(s("0.5")))]);
        let bid = hundredths_text(cents);
        let prefix = |offset: i64| vec![
            Step {
                at: at_second(offset),
                session: MarketSession::Regular,
                input: Input::Mark { bid: Price::parse(&bid).expect("a price"), sane: true },
            },
            Step {
                at: at_second(offset + STEP_S),
                session: MarketSession::Regular,
                input: Input::Fill {
                    side: mandate_domain::Side::Sell,
                    qty: Qty::parse("100").expect("a quantity"),
                    price: Price::parse(&bid).expect("a price"),
                },
            },
        ];
        let stop = |offset: i64| Step {
            at: at_second(offset),
            session: MarketSession::Regular,
            input: Input::AgentStopped { reason: StopReason::OwnerStop },
        };
        let mut plain = prefix(STEP_S);
        plain.push(stop(STEP_S * 3));
        let mut paired = prefix(STEP_S);
        for (index, delta) in [format!("-{amount}"), format!("{amount}")].into_iter().enumerate() {
            paired.push(Step {
                at: at_second(STEP_S * (3 + i64::try_from(index).unwrap_or_default())),
                session: MarketSession::Regular,
                input: Input::AllocationChange {
                    delta_usd: Usd::parse(&delta).expect("a dollar amount"),
                },
            });
        }
        paired.push(stop(STEP_S * 6));
        let carry = |outcomes: Vec<Outcome>| {
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
        prop_assert_eq!(
            carry(run(&mandate, &paired)),
            carry(run(&mandate, &plain)),
            "withdrawing and putting it back cannot change the carry"
        );
    }

    /// §5.4: risk days tile the timeline. For any instant, the day reported contains it, and the day
    /// reported for its end starts exactly there. The oracle is `mandate-time`'s own midnight.
    #[test]
    #[ignore = "pending E6-4"]
    fn risk_days_tile_the_timeline_without_gap_or_overlap(offset_days in 0_i64..700, seconds in 0_i64..86399) {
        let epoch = new_york_midnight(Date::new(2026, 1, 1).expect("a date")).expect("a midnight");
        let at = UtcNanos::from_parts(epoch.secs() + offset_days * 86400 + seconds, 0)
            .expect("an instant");
        let day = risk_day(at).expect("a risk day");
        prop_assert!(day.starts_at.secs() <= at.secs() && at.secs() < day.ends_at.secs());
        prop_assert_eq!(
            day.starts_at,
            new_york_midnight(day.day).expect("a midnight"),
            "a risk day starts at its own New York midnight"
        );
        let next = risk_day(day.ends_at).expect("a risk day");
        prop_assert_eq!(next.starts_at, day.ends_at, "no gap and no overlap");
        prop_assert_eq!(
            u64::from(day.length_s),
            u64::try_from(day.ends_at.secs() - day.starts_at.secs()).expect("a length"),
        );
    }
}

/// MI-3: a latch lifts only by its defined path. Asserted over the generated market that latches
/// something: no `RiskLimitLifted` may appear on a step whose only input was a mark, except a
/// `scale_sizes` rung (which lifts on hysteresis) and a hard-breach clear.
#[test]
#[ignore = "pending E6-4"]
fn no_latch_lifts_except_by_its_defined_path() {
    let mandate = document(&[("/risk/breach_confirm_s", Some(i(0)))]);
    let steps = mark_steps(&[9000, 10500, 10500, 10500]);
    for outcome in run(&mandate, &steps) {
        for event in &outcome.journal {
            if let RiskEvent::RiskLimitLifted { limit, action, .. } = event {
                assert!(
                    matches!(limit, LimitKey::DrawdownRung(_)) && action.is_some(),
                    "only a scale rung lifts on a price alone, got {event:?}"
                );
            }
        }
    }
}

/// §3.1: a completed goal never adds risk. Whatever `on_complete` the owner chose, the mode after the
/// goal completes is at least as strict as it was before, and no limit is lifted by the completion.
#[test]
#[ignore = "pending E6-4"]
fn a_completed_goal_never_adds_risk() {
    for on_complete in ["hold_protected", "disarm_ladder", "release"] {
        let mandate = document(&[("/goal/on_complete", Some(s(on_complete)))]);
        let mut state = open(&mandate);
        let before = state
            .step(&Step {
                at: at_second(STEP_S),
                session: MarketSession::Regular,
                input: Input::Mark {
                    bid: Price::parse("99").expect("a price"),
                    sane: true,
                },
            })
            .expect("an evaluable mark");
        let after = state
            .step(&Step {
                at: at_second(STEP_S * 2),
                session: MarketSession::Regular,
                input: Input::GoalComplete,
            })
            .expect("an evaluable completion");
        assert!(
            after.snapshot.agent_mode >= before.snapshot.agent_mode,
            "`on_complete: {on_complete}` loosened the mode"
        );
        assert!(
            !after
                .journal
                .iter()
                .any(|event| matches!(event, RiskEvent::RiskLimitLifted { .. })),
            "`on_complete: {on_complete}` lifted a limit: {:?}",
            after.journal
        );
    }
}

/// The fixture's own `hysteresis` is what lets a scale rung lift at all, so the base the properties
/// run on must actually carry the three-rung ladder they assume. A generated market over a one-rung
/// ladder would make half the properties vacuous.
#[test]
#[ignore = "pending E6-4"]
fn the_property_base_carries_the_three_rung_ladder() {
    let mandate = mandate_spec::Mandate::parse(&document(&[]))
        .unwrap_or_else(|e| panic!("the property base must parse: {e}"));
    assert_eq!(mandate.risk.drawdown_ladder.len(), 3);
    assert_eq!(mandate.risk.hysteresis, fraction("0.01"));
    assert_eq!(
        mandate.risk.drawdown_ladder.first().map(|rung| rung.action),
        Some(LadderAction::ScaleSizes)
    );
}
