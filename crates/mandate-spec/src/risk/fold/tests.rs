//! The spine's own edges, which the family-R suite reaches only through whole walks: the lift
//! delay's boundary and restart, which rungs a receding drawdown may lift, the risk clock's
//! direction and unit, staleness while R3 is not folded, the clocks crypto and equities count on,
//! the floor's carry, fills, the daily trigger's two actions, and every input left to R3 and R4.
//!
//! A book of 100 shares opened at 100 on an allocation of 10,000, so E = 100 × the bid and the
//! base mandate's levels fall at round bids: rung 0 (2%, `scale_sizes` 0.5) at 98 with its lift
//! level above 99, rung 1 (5%, `exits_only`) at 95, rung 2 (8%, `flatten_and_pause`) at 92, and
//! the floor (10%) at 90.

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::{AgentMode, AssetClass, AssetId, MarketSession, Side};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_time::UtcNanos;
use proptest::prelude::*;

use crate::document::LadderAction;
use crate::risk::limits::tests::base;
use crate::risk::{
    Input, KillScope, Latch, LimitKey, Opening, Outcome, RemovalReason, Restriction, RiskEvent,
    RiskState, SessionClock, Step, StopReason, TriggerReason, UniverseChange,
};
use crate::validate::ValidatedMandate;
use crate::validate::tests::{context, mandate};
use crate::{DecGrammar, SchemaDec, SpecError};

/// 2026-09-21T14:00:00Z, the instant every walk opens at. The clocks here are synthetic, so no
/// calendar reads it.
const OPEN_S: i64 = 1_790_172_000;

/// Every second counts, as crypto counts them (§5.2), and as an equity's would in a session that
/// never closed.
struct Every;

impl SessionClock for Every {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        to.secs()
            .checked_sub(from.secs())
            .and_then(|seconds| u64::try_from(seconds).ok())
            .ok_or(SpecError::ClockWentBackwards)
    }
}

/// No second counts: an equity's session clock while the market is shut.
struct Shut;

impl SessionClock for Shut {
    fn seconds_between(&self, _: UtcNanos, _: UtcNanos) -> Result<u64, SpecError> {
        Ok(0)
    }
}

fn patched(patches: &[(&str, &str)]) -> Result<ValidatedMandate, String> {
    ValidatedMandate::new(mandate(patches)?, &context()?, &[])
        .map_err(|e| format!("{patches:?}: {e}"))
}

/// The base with the daily loss out of the ladder's way, as the R cases isolate it.
fn ladder_only(patches: &[(&str, &str)]) -> Result<ValidatedMandate, String> {
    let mut all = vec![("/risk/max_daily_loss", "\"0.5\"")];
    all.extend_from_slice(patches);
    patched(&all)
}

/// The base as a crypto mandate: both pinned instruments crypto, and the stop-limit offset V-008
/// asks for when crypto is allowed.
fn crypto(patches: &[(&str, &str)]) -> Result<ValidatedMandate, String> {
    let mut all = vec![
        ("/universe/asset_classes", "[\"crypto\"]"),
        ("/universe/pinned_instruments/0/asset_class", "\"crypto\""),
        ("/universe/pinned_instruments/1/asset_class", "\"crypto\""),
        ("/protection/crypto_stop_limit_offset", "\"0.005\""),
        ("/risk/max_daily_loss", "\"0.5\""),
    ];
    all.extend_from_slice(patches);
    patched(&all)
}

fn at(offset_s: i64) -> Result<UtcNanos, String> {
    after(offset_s, 0)
}

fn after(offset_s: i64, nanos: u32) -> Result<UtcNanos, String> {
    let secs = OPEN_S
        .checked_add(offset_s)
        .ok_or("an offset past the clock")?;
    UtcNanos::from_parts(secs, nanos).map_err(|e| e.to_string())
}

fn usd(text: &str) -> Result<Usd, String> {
    Usd::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

fn price(text: &str) -> Result<Price, String> {
    Price::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

fn qty(text: &str) -> Result<Qty, String> {
    Qty::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

fn ratio(text: &str) -> Result<Ratio, String> {
    Ratio::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

/// `held` shares at 100, opened at [`OPEN_S`].
fn opening(
    class: AssetClass,
    held: &str,
    inherited: &str,
    mark_max_age_s: u32,
) -> Result<Opening, String> {
    Ok(Opening {
        position_qty: qty(held)?,
        avg_cost: price("100")?,
        asset_class: class,
        at: at(0)?,
        inherited_loss_usd: usd(inherited)?,
        mark_max_age_s,
    })
}

/// 100 shares of an equity, with a staleness limit no walk here reaches unless it means to.
fn equity() -> Result<Opening, String> {
    opening(AssetClass::UsEquity, "100", "0", 1_000_000)
}

fn quote(offset_s: i64, bid: &str, session: MarketSession, sane: bool) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session,
        input: Input::Mark {
            bid: price(bid)?,
            sane,
        },
    })
}

fn mark(offset_s: i64, bid: &str) -> Result<Step, String> {
    quote(offset_s, bid, MarketSession::Regular, true)
}

fn tick(offset_s: i64) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session: MarketSession::Regular,
        input: Input::Clock,
    })
}

fn fill(offset_s: i64, side: Side, quantity: &str, fill_price: &str) -> Result<Step, String> {
    Ok(Step {
        at: at(offset_s)?,
        session: MarketSession::Regular,
        input: Input::Fill {
            side,
            qty: qty(quantity)?,
            price: price(fill_price)?,
        },
    })
}

fn open<'c>(
    mandate: &ValidatedMandate,
    opening: &Opening,
    clock: &'c dyn SessionClock,
) -> Result<RiskState<'c>, String> {
    RiskState::open(mandate, opening, clock).map_err(|e| format!("open: {e}"))
}

fn walk(state: &mut RiskState<'_>, steps: &[Step]) -> Result<Vec<Outcome>, String> {
    steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            state
                .step(step)
                .map_err(|e| format!("step {}: {e} ({})", index.saturating_add(1), e.code()))
        })
        .collect()
}

fn one(state: &mut RiskState<'_>, step: Result<Step, String>) -> Result<Outcome, String> {
    state
        .step(&step?)
        .map_err(|e| format!("{e} ({})", e.code()))
}

fn triggered(limit: LimitKey, action: LadderAction, reason: Option<TriggerReason>) -> RiskEvent {
    RiskEvent::RiskLimitTriggered {
        limit,
        action,
        reason,
    }
}

fn scale_lifted(index: u8) -> RiskEvent {
    RiskEvent::RiskLimitLifted {
        limit: LimitKey::DrawdownRung(index),
        action: Some(LadderAction::ScaleSizes),
        reason: None,
    }
}

fn moved(from: AgentMode, to: AgentMode) -> RiskEvent {
    RiskEvent::AgentModeApplied { from, to }
}

fn active(state: &RiskState<'_>) -> BTreeMap<u8, u64> {
    state.snapshot().active_rungs.clone()
}

/// §5.5's lift: `scale_lift_after_s` (600) of session time continuously below the lift level. Time on
/// the level itself is not below it and restarts the delay, and the rung lifts at exactly 600 s.
#[test]
fn a_scale_rung_lifts_after_the_whole_delay_below_its_lift_level_and_restarts_on_leaving_it()
-> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    let rung = LimitKey::DrawdownRung(0);
    let first = one(&mut state, mark(1, "98"))?;
    assert_eq!(
        first.journal,
        [triggered(rung, LadderAction::ScaleSizes, None)]
    );
    assert_eq!(first.snapshot.size_factor, ratio("0.5")?);
    assert_eq!(active(&state), BTreeMap::from([(0, 0)]));
    one(&mut state, mark(10, "99.5"))?;
    assert_eq!(
        active(&state),
        BTreeMap::from([(0, 9)]),
        "active time is counted"
    );
    let on_the_level = one(&mut state, mark(300, "99"))?;
    assert!(on_the_level.journal.is_empty());
    one(&mut state, mark(310, "99.5"))?;
    assert!(
        one(&mut state, tick(620))?.journal.is_empty(),
        "the 290 s before the mark on the level do not count towards the delay"
    );
    assert!(
        one(&mut state, tick(909))?.journal.is_empty(),
        "599 s below the lift level is not the delay"
    );
    let lifted = one(&mut state, tick(910))?;
    assert_eq!(lifted.journal, [scale_lifted(0)], "600 s is");
    assert_eq!(lifted.snapshot.size_factor, ratio("1")?);
    assert!(active(&state).is_empty());
    Ok(())
}

/// §5.5 gives the hysteresis lift to `scale_sizes` rungs only; a latched `exits_only` or
/// `flatten_and_pause` rung waits for the owner's acknowledgment (§5.8), however far the drawdown
/// recedes and however long it stays there.
#[test]
fn a_latched_exits_only_or_flatten_rung_does_not_lift_when_the_drawdown_recedes()
-> Result<(), String> {
    let mandate = ladder_only(&[
        ("/risk/breach_confirm_s", "0"),
        ("/risk/scale_lift_after_s", "0"),
    ])?;
    for (bid, severe, restrictions, mode) in [
        (
            "94.5",
            vec![LimitKey::DrawdownRung(1)],
            vec![Restriction::DrawdownExitsOnly],
            AgentMode::ExitsOnly,
        ),
        (
            "91.5",
            vec![LimitKey::DrawdownRung(1), LimitKey::DrawdownRung(2)],
            vec![Restriction::DrawdownExitsOnly, Restriction::DrawdownFlatten],
            AgentMode::Paused,
        ),
    ] {
        let mut state = open(&mandate, &equity()?, &Every)?;
        one(&mut state, mark(1, bid))?;
        let latched: BTreeSet<LimitKey> = severe.iter().copied().collect();
        assert_eq!(state.snapshot().latched, latched, "at {bid}");
        let recovered = walk(
            &mut state,
            &[mark(2, "100")?, tick(86_400)?, mark(86_401, "101")?],
        )?;
        assert_eq!(
            recovered.first().map(|outcome| outcome.journal.as_slice()),
            Some([scale_lifted(0)].as_slice()),
            "the scale rung lifts at {bid}"
        );
        for outcome in &recovered {
            assert!(
                outcome
                    .journal
                    .iter()
                    .all(|event| !matches!(event, RiskEvent::RiskLimitLifted { limit, .. } if latched.contains(limit))),
                "no latched rung lifts at {bid}: {:?}",
                outcome.journal
            );
        }
        let snapshot = state.snapshot();
        assert_eq!(snapshot.latched, latched, "still latched after {bid}");
        assert_eq!(
            snapshot.restrictions,
            restrictions.into_iter().collect::<BTreeSet<_>>()
        );
        assert_eq!(snapshot.agent_mode, mode);
    }
    Ok(())
}

/// The risk clock is monotone and whole-second (§5.2): a step at the previous instant folds with no
/// time passing, one before it is `clock_went_backwards`, and one between two seconds is refused.
/// Each refusal, and a refused oversale, leaves the state exactly as it was, its time included.
#[test]
fn the_risk_clock_is_monotone_and_whole_second_and_a_refused_step_changes_nothing()
-> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    let same = one(&mut state, mark(0, "98"))?;
    assert_eq!(
        same.journal,
        [triggered(
            LimitKey::DrawdownRung(0),
            LadderAction::ScaleSizes,
            None
        )],
        "a step at the opening instant folds"
    );
    one(&mut state, mark(10, "99"))?;
    one(&mut state, mark(10, "99"))?;
    let before = state.clone();
    let between = Step {
        at: after(20, 500_000_000)?,
        ..mark(20, "99")?
    };
    for (step, code) in [
        (mark(9, "99")?, "clock_went_backwards"),
        (between, "invalid_input"),
        (
            fill(20, Side::Sell, "100.000000001", "99")?,
            "invalid_input",
        ),
    ] {
        let refused = state.step(&step).map(|_| ()).map_err(|e| e.code());
        assert_eq!(refused, Err(code), "{step:?}");
        assert_eq!(state, before, "{step:?} changed nothing");
        assert_eq!(state.snapshot(), before.snapshot());
    }
    assert!(
        one(&mut state, tick(15)).is_ok(),
        "the refused step at 20 s did not move the clock"
    );
    let off_the_clock = Opening {
        at: after(0, 1)?,
        ..equity()?
    };
    assert_eq!(
        RiskState::open(&mandate, &off_the_clock, &Every)
            .map(|_| ())
            .map_err(|e| e.code()),
        Err("invalid_input")
    );
    Ok(())
}

/// Staleness is R3's: a held instrument whose mark reaches `mark_max_age_s` of session time, or
/// whose counted mark fails its checks, is `unimplemented` rather than a step with no
/// `stale_mark`. A sane mark resets the age and a fill does not, an equity's extended-hours mark is
/// not counted, and a book that was flat for the interval has nothing to go stale.
#[test]
fn a_held_instrument_going_stale_is_not_folded_yet_and_a_flat_book_never_goes_stale()
-> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let held = opening(AssetClass::UsEquity, "100", "0", 120)?;
    let unimplemented = |state: &mut RiskState<'_>, step: Step| -> Result<(), String> {
        let before = state.clone();
        assert_eq!(
            state.step(&step).map(|_| ()).map_err(|e| e.code()),
            Err("unimplemented"),
            "{step:?}"
        );
        assert_eq!(*state, before, "{step:?} changed nothing");
        Ok(())
    };
    let mut state = open(&mandate, &held, &Every)?;
    one(&mut state, tick(119))?;
    unimplemented(&mut state, tick(120)?)?;
    one(&mut state, mark(120, "100"))?;
    one(&mut state, fill(200, Side::Buy, "1", "100"))?;
    unimplemented(&mut state, tick(240)?)?;
    unimplemented(&mut state, fill(240, Side::Sell, "101", "100")?)?;
    one(&mut state, tick(239))?;
    unimplemented(
        &mut state,
        quote(239, "100", MarketSession::Regular, false)?,
    )?;
    one(
        &mut state,
        quote(239, "100", MarketSession::AfterHours, false),
    )?;

    let mut shut = open(&mandate, &held, &Shut)?;
    one(&mut shut, tick(100_000))?;

    let flat = opening(AssetClass::UsEquity, "0", "0", 120)?;
    let mut state = open(&mandate, &flat, &Every)?;
    one(&mut state, tick(100_000))?;
    one(
        &mut state,
        quote(100_001, "100", MarketSession::Regular, false),
    )?;
    one(&mut state, fill(100_002, Side::Buy, "10", "100"))?;
    one(&mut state, tick(100_121))?;
    unimplemented(&mut state, tick(100_122)?)?;
    Ok(())
}

/// Crypto counts every mark and every second (§5.2): its lift delay runs on wall seconds even when
/// the caller's clock counts none, and a mark in any session moves E. An equity counts only
/// regular-session marks and only the clock's seconds for its lift, while confirmation runs on wall
/// seconds for both (§5.6).
#[test]
fn crypto_counts_every_mark_and_second_and_an_equity_its_sessions_only() -> Result<(), String> {
    let coins = crypto(&[])?;
    let bitcoin = opening(AssetClass::Crypto, "100", "0", 1_000_000)?;
    let mut state = open(&coins, &bitcoin, &Shut)?;
    let overnight = one(&mut state, quote(1, "98", MarketSession::Overnight, true))?;
    assert_eq!(overnight.snapshot.agent_equity, usd("9800")?);
    one(&mut state, quote(2, "99.5", MarketSession::Crypto, true))?;
    assert_eq!(active(&state), BTreeMap::from([(0, 1)]));
    assert_eq!(one(&mut state, tick(602))?.journal, [scale_lifted(0)]);

    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Shut)?;
    for session in [
        MarketSession::PreMarket,
        MarketSession::AfterHours,
        MarketSession::Overnight,
    ] {
        let ignored = one(&mut state, quote(1, "90", session, true))?;
        assert_eq!(ignored.snapshot.agent_equity, usd("10000")?, "{session:?}");
        assert!(ignored.journal.is_empty(), "{session:?}");
    }
    one(&mut state, mark(1, "98"))?;
    one(&mut state, mark(2, "99.5"))?;
    assert!(
        one(&mut state, tick(100_000))?.journal.is_empty(),
        "no session second passed"
    );
    assert_eq!(active(&state), BTreeMap::from([(0, 0)]));

    let mut state = open(&base()?, &equity()?, &Shut)?;
    one(&mut state, mark(1, "94.5"))?;
    let confirmed = one(&mut state, tick(61))?;
    assert!(
        confirmed.journal.contains(&triggered(
            LimitKey::DrawdownRung(1),
            LadderAction::ExitsOnly,
            None
        )),
        "60 wall seconds confirm with the market shut: {:?}",
        confirmed.journal
    );
    Ok(())
}

/// The floor is C × (1 − `max_loss_from_allocation`) + L (§5.7): an inherited loss of 500 raises it
/// from 9,000 to 9,500, so a fall to 9,490 latches it, pauses the agent, and fires the agent's kill
/// switch, where without the carry it would not.
#[test]
fn the_floor_carries_the_inherited_loss() -> Result<(), String> {
    let mandate = ladder_only(&[("/risk/breach_confirm_s", "0")])?;
    for (inherited, floors) in [("500", true), ("0", false)] {
        let carried = opening(AssetClass::UsEquity, "100", inherited, 1_000_000)?;
        let mut state = open(&mandate, &carried, &Every)?;
        assert_eq!(state.snapshot().inherited_loss, usd(inherited)?);
        let outcome = one(&mut state, mark(1, "94.9"))?;
        let floor = [
            triggered(LimitKey::LifetimeFloor, LadderAction::FlattenAndPause, None),
            RiskEvent::KillSwitchActivated {
                scope: KillScope::Agent,
                initiator: LimitKey::LifetimeFloor,
            },
        ];
        assert_eq!(
            outcome.journal.windows(2).any(|pair| pair == floor),
            floors,
            "inherited {inherited}: {:?}",
            outcome.journal
        );
        assert_eq!(
            outcome
                .snapshot
                .restrictions
                .contains(&Restriction::LifetimeFloor),
            floors
        );
        assert_eq!(
            outcome.snapshot.agent_mode,
            if floors {
                AgentMode::Paused
            } else {
                AgentMode::ExitsOnly
            }
        );
    }
    Ok(())
}

/// A fill moves the sub-ledger at its own price while E stays marked at the last sane mark (§5.2):
/// buying 10 at 90 with the mark at 100 adds 100 to E. A sale at the mark moves nothing. A fill is
/// not a quote, so a hard breach neither arms, latches, nor clears on one.
#[test]
fn a_fill_moves_equity_by_its_price_against_the_mark_and_is_not_a_quote() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    let bought = one(&mut state, fill(1, Side::Buy, "10", "90"))?;
    assert_eq!(bought.snapshot.agent_equity, usd("10100")?);
    assert_eq!(bought.snapshot.high_water_mark, usd("10100")?);
    let sold = one(&mut state, fill(2, Side::Sell, "110", "100"))?;
    assert_eq!(sold.snapshot.agent_equity, usd("10100")?);
    one(&mut state, mark(3, "1"))?;
    assert_eq!(
        state.snapshot().agent_equity,
        usd("10100")?,
        "a flat book has no market value"
    );

    let severe = ladder_only(&[("/risk/breach_confirm_s", "60")])?;
    let hard = LimitKey::DrawdownRung(1);
    let pending = triggered(
        hard,
        LadderAction::ExitsOnly,
        Some(TriggerReason::HardBreachPending),
    );
    let mut state = open(&severe, &equity()?, &Every)?;
    let bought_dear = one(&mut state, fill(1, Side::Buy, "100", "107"))?;
    assert_eq!(
        bought_dear.snapshot.agent_equity,
        usd("9300")?,
        "7% down, past rung 1's 6.25% hard level"
    );
    assert!(
        !bought_dear.journal.contains(&pending)
            && !bought_dear
                .snapshot
                .restrictions
                .contains(&Restriction::HardBreach),
        "a fill does not arm a hard breach: {:?}",
        bought_dear.journal
    );
    let mut state = open(&severe, &equity()?, &Every)?;
    let armed = one(&mut state, mark(1, "93.5"))?;
    assert!(armed.journal.contains(&pending), "{:?}", armed.journal);
    let fills = walk(
        &mut state,
        &[
            fill(20, Side::Buy, "1", "93.5")?,
            fill(21, Side::Sell, "1", "200")?,
        ],
    )?;
    assert_eq!(
        fills.get(1).map(|outcome| outcome.snapshot.agent_equity),
        Some(usd("9456.5")?),
        "below the hard level after the dear sale"
    );
    for outcome in &fills {
        assert!(
            outcome.journal.is_empty()
                && outcome
                    .snapshot
                    .restrictions
                    .contains(&Restriction::HardBreach),
            "a fill neither latches nor clears the hard breach: {:?}",
            outcome.journal
        );
    }
    let latched = one(&mut state, mark(22, "92.5"))?;
    assert!(
        latched.journal.contains(&triggered(
            hard,
            LadderAction::ExitsOnly,
            Some(TriggerReason::HardTrigger)
        )),
        "the second quote latches: {:?}",
        latched.journal
    );
    Ok(())
}

/// The daily loss confirms like any limit and applies the mandate's `daily_loss_action` (§5.4): its
/// `exits_only` restricts, and its `flatten_and_pause` fires the agent's kill switch and pauses,
/// which `Restriction::mode` alone would read as `exits_only`. At 1.25x it takes the hard trigger.
#[test]
fn the_daily_loss_confirms_and_applies_its_action_and_takes_the_hard_trigger() -> Result<(), String>
{
    let daily = LimitKey::MaxDailyLoss;
    for (action, mode, killed) in [
        ("exits_only", AgentMode::ExitsOnly, false),
        ("flatten_and_pause", AgentMode::Paused, true),
    ] {
        let mandate = patched(&[
            ("/risk/max_daily_loss", "\"0.01\""),
            ("/risk/daily_loss_action", &format!("\"{action}\"")),
        ])?;
        let mut state = open(&mandate, &equity()?, &Every)?;
        let pending = one(&mut state, mark(1, "99"))?;
        assert_eq!(pending.pending, BTreeSet::from([daily]), "{action}");
        assert!(pending.journal.is_empty(), "{action}");
        assert!(
            one(&mut state, tick(60))?.journal.is_empty(),
            "59 s is not 60"
        );
        let confirmed = one(&mut state, tick(61))?;
        let mut expected = vec![triggered(daily, action_of(action)?, None)];
        if killed {
            expected.push(RiskEvent::KillSwitchActivated {
                scope: KillScope::Agent,
                initiator: daily,
            });
        }
        expected.push(moved(AgentMode::Normal, mode));
        assert_eq!(confirmed.journal, expected, "{action}");
        assert!(confirmed.pending.is_empty());
        assert_eq!(confirmed.snapshot.latched, BTreeSet::from([daily]));
        assert_eq!(
            confirmed.snapshot.restrictions,
            BTreeSet::from([Restriction::DailyLoss])
        );
    }
    let mandate = patched(&[("/risk/max_daily_loss", "\"0.01\"")])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    let armed = one(&mut state, mark(1, "98.75"))?;
    assert_eq!(
        armed.journal,
        [
            triggered(
                daily,
                LadderAction::ExitsOnly,
                Some(TriggerReason::HardBreachPending)
            ),
            moved(AgentMode::Normal, AgentMode::ExitsOnly),
        ]
    );
    assert!(
        one(&mut state, mark(10, "98.75"))?.journal.is_empty(),
        "9 s is short of the 10 s wait"
    );
    let latched = one(&mut state, mark(11, "98.75"))?;
    assert_eq!(
        latched.journal,
        [triggered(
            daily,
            LadderAction::ExitsOnly,
            Some(TriggerReason::HardTrigger)
        )]
    );
    assert_eq!(
        latched.snapshot.restrictions,
        BTreeSet::from([Restriction::DailyLoss])
    );
    Ok(())
}

fn action_of(action: &str) -> Result<LadderAction, String> {
    match action {
        "exits_only" => Ok(LadderAction::ExitsOnly),
        "flatten_and_pause" => Ok(LadderAction::FlattenAndPause),
        other => Err(format!("`{other}` is not a daily-loss action")),
    }
}

/// What R3 and R4 fold is `unimplemented`, never a silent answer, and leaves the state as it was: the
/// risk day, a universe change, acknowledgments, allocation changes, floor loosening, retirement,
/// goal completion, and a `profit_stop` goal at opening.
#[test]
fn every_input_left_to_later_slices_is_unimplemented_and_changes_nothing() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let mut state = open(&mandate, &equity()?, &Every)?;
    one(&mut state, mark(1, "98"))?;
    let before = state.clone();
    let instrument =
        AssetId::parse("7b4a1c2e-2222-4a2b-9c3d-000000000002").map_err(|e| e.to_string())?;
    let loosened = SchemaDec::parse("0.2", DecGrammar::OpenFraction).map_err(|e| e.to_string())?;
    for input in [
        Input::RiskDayStarted,
        Input::UniverseChanged {
            instrument,
            change: UniverseChange::Removed,
            reason: RemovalReason::ThesisExpired,
        },
        Input::OwnerAcknowledged {
            restriction: Latch::DrawdownLadder,
        },
        Input::AllocationChange {
            delta_usd: usd("100")?,
        },
        Input::FloorLoosened {
            new_max_loss_from_allocation: loosened,
            confirmed_at: at(1)?,
            independent_approval: true,
        },
        Input::AgentStopped {
            reason: StopReason::OwnerStop,
        },
        Input::GoalComplete,
    ] {
        let step = Step {
            at: at(2)?,
            session: MarketSession::Regular,
            input,
        };
        assert_eq!(
            state.step(&step).map(|_| ()).map_err(|e| e.code()),
            Err("unimplemented"),
            "{step:?}"
        );
        assert_eq!(state, before, "{step:?} changed nothing");
    }
    let profit_stop = patched(&[(
        "/goal",
        "{\"type\": \"profit_stop\", \"profit_level\": \"0.1\", \"end_date\": null}",
    )])?;
    assert_eq!(
        RiskState::open(&profit_stop, &equity()?, &Every)
            .map(|_| ())
            .map_err(|e| e.code()),
        Err("unimplemented")
    );
    Ok(())
}

/// A negative inherited loss would lower the floor below C × (1 − the fraction), so it is refused.
#[test]
fn a_negative_inherited_loss_is_refused() -> Result<(), String> {
    let lowered = opening(AssetClass::UsEquity, "100", "-1", 1_000_000)?;
    assert_eq!(
        RiskState::open(&base()?, &lowered, &Every)
            .map(|_| ())
            .map_err(|e| e.code()),
        Err("invalid_input")
    );
    Ok(())
}

/// The clock is not state (DEC-167 item 5): two states that folded the same inputs are equal whichever
/// clock each counts on, and a different input makes them differ.
#[test]
fn states_are_equal_whichever_clock_they_hold() -> Result<(), String> {
    let mandate = ladder_only(&[])?;
    let every = Every;
    let shut = Shut;
    let mut left = open(&mandate, &equity()?, &every)?;
    let mut right = open(&mandate, &equity()?, &shut)?;
    assert_eq!(left, right);
    one(&mut left, mark(0, "99"))?;
    one(&mut right, mark(0, "99"))?;
    assert_eq!(left, right);
    one(&mut right, mark(0, "98"))?;
    assert_ne!(left, right);
    Ok(())
}

/// A state prints what it folded and what it reports, and not the clock it borrows, which has no
/// `Debug` of its own; `assert_eq!` on two states relies on this to say how they differ.
#[test]
fn a_state_prints_its_fold_and_snapshot_and_not_its_clock() -> Result<(), String> {
    let state = open(&ladder_only(&[])?, &equity()?, &Every)?;
    let printed = format!("{state:?}");
    assert!(printed.starts_with("RiskState { fold: Fold {"), "{printed}");
    assert!(
        printed.contains(&format!("snapshot: {:?}", state.snapshot())),
        "{printed}"
    );
    assert!(printed.ends_with(", .. }"), "{printed}");
    assert!(!printed.contains("clock"), "{printed}");
    Ok(())
}

/// One input to the lift property: a mark at one of three bids after a gap.
#[derive(Debug, Clone, Copy)]
enum Level {
    /// 97: rung 0 is hit.
    Hit,
    /// 98.5: between the rung and its lift level.
    Between,
    /// 99.5: below the lift level.
    Below,
}

impl Level {
    fn bid(self) -> &'static str {
        match self {
            Self::Hit => "97",
            Self::Between => "98.5",
            Self::Below => "99.5",
        }
    }
}

proptest! {
    /// §5.5's lift, read declaratively: the rung is active from any mark that hits it until the
    /// first mark that ends a run of marks below the lift level spanning at least
    /// `scale_lift_after_s`, measured from the run's first mark. The fold credits intervals one at a
    /// time; the oracle only remembers when the run began.
    #[test]
    fn a_scale_rung_is_active_exactly_as_long_as_the_run_rule_says(
        marks in prop::collection::vec(
            (1_i64..=400, prop::sample::select(vec![Level::Hit, Level::Between, Level::Below])),
            1..40,
        ),
    ) {
        let mandate = ladder_only(&[]).map_err(TestCaseError::fail)?;
        let mut state = open(&mandate, &equity().map_err(TestCaseError::fail)?, &Every)
            .map_err(TestCaseError::fail)?;
        let mut now = 0_i64;
        let mut oracle_active = false;
        let mut run_began: Option<i64> = None;
        for (gap, level) in marks {
            now = now.saturating_add(gap);
            match level {
                Level::Hit => {
                    oracle_active = true;
                    run_began = None;
                }
                Level::Between => run_began = None,
                Level::Below => {
                    let began = *run_began.get_or_insert(now);
                    if oracle_active && now.saturating_sub(began) >= 600 {
                        oracle_active = false;
                    }
                }
            }
            let outcome = one(&mut state, mark(now, level.bid())).map_err(TestCaseError::fail)?;
            prop_assert_eq!(
                outcome.snapshot.active_rungs.contains_key(&0),
                oracle_active,
                "at {} s after {:?}: {:?}", now, level, outcome.journal
            );
        }
    }
}
