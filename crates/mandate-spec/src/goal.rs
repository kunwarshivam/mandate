//! Goal completion ([mandate spec §3.1](../../../docs/specs/mandate.md#31-goals-and-stop-conditions-dec-46-dec-59)).
//!
//! A `profit_stop` is **not** decided here: §3.1 confirms it by breach time inside the risk state
//! (§5.6), so [`status`] returns [`GoalStatus::ConfirmedInRiskState`] rather than `Running`. Returning
//! `Running` would invite a caller to decide the same condition twice, in two places, with two answers.

use mandate_num::{NumError, Price, Qty, Usd};
use mandate_time::{UtcNanos, new_york_midnight};

use crate::document::{Goal, OnComplete, Pointer};
use crate::risk::{GoalReason, StopReason, ThenAction};
use crate::validate::ValidatedMandate;
use crate::{SchemaDec, SpecError};

/// What the agent holds and what the market offers, which is all §3.1 needs to say whether a goal is
/// done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalInputs {
    pub now: UtcNanos,
    pub position_qty: Qty,
    /// The sum of the agent's buy fills in the goal instrument, fees included. Sales never reduce it
    /// (§3.1), so it is never negative; a negative sum is [`SpecError::InvalidInput`], because it
    /// would hold a goal open past its spend.
    pub goal_spent_usd: Usd,
    pub min_order_usd: Usd,
    /// The instrument's quantity increment, as the reference cases carry it: a decimal (`1` for whole
    /// shares, `0.0001` for BTC), not the whole-or-fractional grid `mandate_num::ShareIncrement`
    /// describes, which cannot express `0.0001` and would make MC-L02's dust remainder look tradable.
    /// Zero is [`SpecError::InvalidInput`].
    pub qty_increment: Qty,
    /// The ask a remainder is valued at against the minimum order. It must come from a quote that
    /// passed §5.6's sane-and-fresh filter: one bad tick far below the market would make any
    /// remainder look worth less than the minimum order and finish the goal, and a `release` goal then
    /// cancels its protection and retires the agent. This function cannot tell a bad tick from a real
    /// one, so the caller that feeds it guarantees the filter: the order path's goal evaluation in
    /// `mandate-risk` (stream G), which reads the same sane quote the risk state marks with.
    pub ask: Price,
}

/// Where a goal stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalStatus {
    Running,
    /// A `profit_stop`: the risk state confirms it (§3.1, §5.6).
    ConfirmedInRiskState,
    Done {
        reason: GoalReason,
        then: ThenAction,
        stop_reason: StopReason,
    },
}

/// Whether the goal is done, why, and what follows (§3.1).
///
/// An `end_date` ends the goal at 00:00 America/New_York **after** that date, so the last risk day of
/// the goal is the date itself. It is judged first, then an accumulate goal's target and then its spend,
/// as `reference/mandate/ref.py`'s `goal_status` does, so a goal past its date is done for its date
/// whatever else holds (DEC-167 item 4).
///
/// An `end_date` whose closing midnight the calendar cannot state is not an answer, and it is an error
/// unless an accumulate goal is done for its target or its spend anyway: a goal whose target is met is
/// done, and no unrepresentable date can hold it open.
pub fn status(mandate: &ValidatedMandate, inputs: &GoalInputs) -> Result<GoalStatus, SpecError> {
    let goal = &mandate.mandate().goal;
    let ended = end_date_passed(goal, inputs.now);
    match goal {
        Goal::ProfitStop { .. } => Ok(if ended? {
            GoalStatus::Done {
                reason: GoalReason::EndDate,
                then: ThenAction::DiscretionaryExitAllThenRetire,
                stop_reason: StopReason::EndDate,
            }
        } else {
            GoalStatus::ConfirmedInRiskState
        }),
        Goal::Continuous { on_complete, .. } => {
            Ok(done_if(ended?.then_some(GoalReason::EndDate), *on_complete))
        }
        Goal::Accumulate {
            target_qty,
            max_spend_usd,
            on_complete,
            ..
        } => {
            if inputs.goal_spent_usd.is_negative() {
                return Err(SpecError::InvalidInput {
                    what: "goal_spent_usd, a sum of buy fills, which is never negative",
                });
            }
            if inputs.qty_increment.is_zero() {
                return Err(SpecError::InvalidInput {
                    what: "qty_increment, which must be above zero",
                });
            }
            if matches!(ended, Ok(true)) {
                return Ok(done_if(Some(GoalReason::EndDate), *on_complete));
            }
            let reason = if quantity_exhausted(target_qty, inputs)? {
                Some(GoalReason::TargetQty)
            } else if spend_exhausted(max_spend_usd, inputs)? {
                Some(GoalReason::MaxSpend)
            } else {
                ended?;
                None
            };
            Ok(done_if(reason, *on_complete))
        }
    }
}

/// `Done` with the owner's `on_complete` when there is a reason, `Running` when there is none.
fn done_if(reason: Option<GoalReason>, on_complete: OnComplete) -> GoalStatus {
    match reason {
        Some(reason) => GoalStatus::Done {
            reason,
            then: ThenAction::Applied(on_complete),
            stop_reason: StopReason::GoalComplete,
        },
        None => GoalStatus::Running,
    }
}

/// Whether `now` is at or after 00:00 America/New_York on the day after `end_date`: the date is the
/// goal's last risk day, and the midnight that closes it is the first instant the goal is over.
fn end_date_passed(goal: &Goal, now: UtcNanos) -> Result<bool, SpecError> {
    match goal.end_date() {
        Some(end_date) => Ok(now >= new_york_midnight(end_date.next()?)?),
        None => Ok(false),
    }
}

/// Whether what remains of `target_qty` is below one increment, or worth less than the minimum order
/// at the ask. A position at or past the target leaves nothing, which is below any increment.
fn quantity_exhausted(target_qty: &SchemaDec, inputs: &GoalInputs) -> Result<bool, SpecError> {
    let target = Qty::parse(target_qty.as_str()).map_err(out_of_range("/goal/target_qty"))?;
    let remaining = match target.checked_sub(inputs.position_qty) {
        Ok(remaining) => remaining,
        Err(NumError::Negative) => return Ok(true),
        Err(other) => return Err(other.into()),
    };
    Ok(remaining < inputs.qty_increment || remaining.notional(inputs.ask)? < inputs.min_order_usd)
}

/// Whether what remains of `max_spend_usd` is below the minimum order. Goal spend counts fees and is
/// never reduced by a sale, which is the caller's sum; spend past the cap leaves a negative remainder,
/// which is below any minimum.
fn spend_exhausted(max_spend_usd: &SchemaDec, inputs: &GoalInputs) -> Result<bool, SpecError> {
    let cap = max_spend_usd
        .to_usd()
        .map_err(out_of_range("/goal/max_spend_usd"))?;
    Ok(cap.checked_sub(inputs.goal_spent_usd)? < inputs.min_order_usd)
}

fn out_of_range(path: &'static str) -> impl Fn(NumError) -> SpecError {
    move |cause| SpecError::OutOfRange {
        path: Pointer::new(path),
        cause,
    }
}

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
#[allow(
    dead_code,
    clippy::expect_used,
    reason = "the integration tests' mandate builder, shared rather than copied: it carries helpers these tests do not use, and it is test code"
)]
mod common;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::error::Error;

    use mandate_canon::Value;
    use mandate_domain::Environment;
    use mandate_num::{Price, Qty, Usd};
    use mandate_time::{Date, UtcNanos};

    use super::common::{arr, obj, s, with_all};
    use super::{GoalInputs, GoalStatus, quantity_exhausted, spend_exhausted, status};
    use crate::document::{OnComplete, Pointer, ProvenanceMap};
    use crate::risk::{GoalReason, StopReason, ThenAction};
    use crate::validate::{ValidatedMandate, ValidationContext};
    use crate::{DecGrammar, Mandate, SchemaDec, SpecError};

    type Checked = Result<(), Box<dyn Error>>;

    const GOAL_INSTRUMENT: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";

    fn inputs(position: &str, ask: &str) -> Result<GoalInputs, Box<dyn Error>> {
        Ok(GoalInputs {
            now: UtcNanos::parse("2026-09-21T15:00:00.000000000Z")?,
            position_qty: Qty::parse(position)?,
            goal_spent_usd: Usd::ZERO,
            min_order_usd: Usd::parse("1")?,
            qty_increment: Qty::parse("0.0001")?,
            ask: Price::parse(ask)?,
        })
    }

    fn at(now: &str, position: &str, spent: &str) -> Result<GoalInputs, Box<dyn Error>> {
        Ok(GoalInputs {
            now: UtcNanos::parse(now)?,
            goal_spent_usd: Usd::parse(spent)?,
            ..inputs(position, "55000")?
        })
    }

    fn validated(changes: &[(&str, Option<Value>)]) -> Result<ValidatedMandate, Box<dyn Error>> {
        let context = ValidationContext {
            account_equity_usd: Usd::parse("25000")?,
            other_allocations_usd: Usd::ZERO,
            validation_date: Date::parse("2026-09-20")?,
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
        };
        Ok(ValidatedMandate::new(
            Mandate::parse(&with_all(changes))?,
            &context,
            &[],
        )?)
    }

    /// The `btc_accumulator` figures of `tests/goal.rs`: target 0.15, max spend 9000, the given end
    /// date, `hold_protected`, and a universe pinned to the goal instrument (V-003).
    fn accumulator(end_date: &str) -> Result<ValidatedMandate, Box<dyn Error>> {
        validated(&[
            ("/goal/type", Some(s("accumulate"))),
            ("/goal/instrument", Some(s(GOAL_INSTRUMENT))),
            ("/goal/target_qty", Some(s("0.15"))),
            ("/goal/max_avg_price", Some(s("58000"))),
            ("/goal/max_spend_usd", Some(s("9000"))),
            ("/goal/end_date", Some(s(end_date))),
            ("/goal/on_complete", Some(s("hold_protected"))),
            (
                "/universe/pinned_instruments",
                Some(arr(vec![obj(vec![
                    ("asset_id", s(GOAL_INSTRUMENT)),
                    ("symbol", s("AAA")),
                    ("asset_class", s("us_equity")),
                ])])),
            ),
        ])
    }

    fn done(reason: GoalReason) -> GoalStatus {
        GoalStatus::Done {
            reason,
            then: ThenAction::Applied(OnComplete::HoldProtected),
            stop_reason: StopReason::GoalComplete,
        }
    }

    /// Past its end date a goal is done for its date, even with its target met or its spend gone, as
    /// `reference/mandate/ref.py` judges it (DEC-167 item 4). The day before, the target and the spend
    /// decide.
    #[test]
    fn the_end_date_outranks_the_target_and_the_spend() -> Checked {
        let goal = accumulator("2026-12-31")?;
        let after = "2027-01-01T05:00:00.000000000Z";
        let before = "2026-12-31T15:00:00.000000000Z";
        assert_eq!(
            status(&goal, &at(after, "0.15", "8300")?)?,
            done(GoalReason::EndDate)
        );
        assert_eq!(
            status(&goal, &at(after, "0.14", "8999.5")?)?,
            done(GoalReason::EndDate)
        );
        assert_eq!(
            status(&goal, &at(before, "0.15", "8300")?)?,
            done(GoalReason::TargetQty)
        );
        assert_eq!(
            status(&goal, &at(before, "0.14", "8999.5")?)?,
            done(GoalReason::MaxSpend)
        );
        Ok(())
    }

    /// The goal ends at 00:00 New York after its end date, which is 04:00Z after a spring-forward
    /// date and 05:00Z after a fall-back one: a day is not always 86,400 s (§3.1, §5.4).
    #[test]
    fn the_end_date_closes_at_new_york_midnight_across_daylight_saving() -> Checked {
        for (end_date, closes) in [
            ("2027-03-14", "2027-03-15T04:00:00.000000000Z"),
            ("2027-03-15", "2027-03-16T04:00:00.000000000Z"),
            ("2027-11-07", "2027-11-08T05:00:00.000000000Z"),
            ("2027-11-08", "2027-11-09T05:00:00.000000000Z"),
        ] {
            let goal = accumulator(end_date)?;
            let closes = UtcNanos::parse(closes)?;
            let last = UtcNanos::from_parts(closes.secs().saturating_sub(1), 0)?;
            let running = GoalInputs {
                now: last,
                ..inputs("0.1", "55000")?
            };
            let ended = GoalInputs {
                now: closes,
                ..inputs("0.1", "55000")?
            };
            assert_eq!(status(&goal, &running)?, GoalStatus::Running, "{end_date}");
            assert_eq!(
                status(&goal, &ended)?,
                done(GoalReason::EndDate),
                "{end_date}"
            );
        }
        Ok(())
    }

    /// An end date whose closing midnight the calendar cannot state holds no goal open whose target
    /// is met, and decides nothing else: a goal that is not done for another reason is an error.
    #[test]
    fn an_unstatable_end_date_blocks_nothing_that_is_done() -> Checked {
        let goal = accumulator("9999-12-31")?;
        let now = "2026-09-21T15:00:00.000000000Z";
        assert_eq!(
            status(&goal, &at(now, "0.15", "8300")?)?,
            done(GoalReason::TargetQty)
        );
        assert_eq!(
            status(&goal, &at(now, "0.1", "8999.5")?)?,
            done(GoalReason::MaxSpend)
        );
        assert!(status(&goal, &at(now, "0.1", "5000")?).is_err());
        Ok(())
    }

    /// Goal spend is a sum of buy fills, never negative; a negative one would hold the goal open past
    /// its spend, so it is refused rather than read (rule 3).
    #[test]
    fn a_negative_goal_spend_is_refused() -> Checked {
        let goal = accumulator("2026-12-31")?;
        assert!(matches!(
            status(&goal, &at("2026-09-21T15:00:00.000000000Z", "0.15", "-0.01")?),
            Err(SpecError::InvalidInput { what }) if what.contains("goal_spent_usd")
        ));
        assert!(status(&goal, &at("2026-09-21T15:00:00.000000000Z", "0.1", "0")?).is_ok());
        Ok(())
    }

    /// The two inputs a caller can get wrong are checked before the end date is read, so past its date
    /// a zero increment is refused just as a negative spend is, rather than one erroring and the other
    /// finishing the goal (#279 review round 2, nit 1).
    #[test]
    fn a_bad_input_is_refused_past_the_end_date_as_before_it() -> Checked {
        let goal = accumulator("2026-12-31")?;
        for now in [
            "2026-09-21T15:00:00.000000000Z",
            "2027-01-01T05:00:00.000000000Z",
        ] {
            let mut zero = at(now, "0.1", "5000")?;
            zero.qty_increment = Qty::ZERO;
            assert!(
                matches!(status(&goal, &zero), Err(SpecError::InvalidInput { what }) if what.contains("qty_increment")),
                "a zero increment at {now}"
            );
            assert!(
                matches!(status(&goal, &at(now, "0.1", "-0.01")?), Err(SpecError::InvalidInput { what }) if what.contains("goal_spent_usd")),
                "a negative spend at {now}"
            );
        }
        Ok(())
    }

    /// A `max_spend_usd` a `Usd` cannot hold is `out_of_range` naming its own pointer (DEC-128 item 4).
    #[test]
    fn a_spend_cap_a_usd_cannot_hold_names_its_pointer() -> Checked {
        let cap = SchemaDec::parse(
            "1234567890123456789012345678.1234567890123456789012345678",
            DecGrammar::PositiveDecimal,
        )?;
        let answer = spend_exhausted(&cap, &inputs("0.1", "55000")?);
        assert!(
            matches!(&answer, Err(SpecError::OutOfRange { path, .. }) if *path == Pointer::new("/goal/max_spend_usd")),
            "{answer:?}"
        );
        Ok(())
    }

    /// A remainder worth exactly the minimum order can still be bought; a cent less cannot (§3.1).
    ///
    /// 0.01 of a 0.15 target remains, so at an ask of 100 it is worth 1.00, the minimum itself.
    #[test]
    fn a_remainder_worth_exactly_the_minimum_order_is_not_done() -> Checked {
        let target = SchemaDec::parse("0.15", DecGrammar::PositiveDecimal)?;
        assert!(!quantity_exhausted(&target, &inputs("0.14", "100")?)?);
        assert!(quantity_exhausted(&target, &inputs("0.14", "99.99")?)?);
        Ok(())
    }

    /// A position past the target leaves nothing to buy, whatever the ask (§3.1).
    #[test]
    fn a_position_past_the_target_is_done() -> Checked {
        let target = SchemaDec::parse("0.15", DecGrammar::PositiveDecimal)?;
        assert!(quantity_exhausted(&target, &inputs("0.2", "1000000")?)?);
        Ok(())
    }
}
