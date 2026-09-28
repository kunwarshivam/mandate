//! The limit conditions of §5.2 and §5.6: each rung, the daily loss, and the lifetime floor, soft
//! and at the 1.25x hard level, compared exactly on the figures a state reports.
//!
//! They read only the mandate's levels and five figures, so the public [`conditions`](super::conditions)
//! and the fold behind [`RiskState`](super::RiskState) make the same comparisons by construction.

use std::collections::BTreeMap;

use mandate_num::{Usd, UsdExact};

use super::{HARD_TRIGGER_MULTIPLE, LimitKey, Snapshot, decimal, rung_index};
use crate::validate::ValidatedMandate;
use crate::{SchemaDec, SpecError};

/// The levels the conditions compare against (§5.4, §5.5, §5.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Limits {
    /// Each rung's `at`, in ladder order.
    pub(super) levels: Vec<UsdExact>,
    max_daily_loss: UsdExact,
    /// The one level a version can change while a state lives: only a loosening version, which is
    /// the one path that lifts the floor (§5.7).
    pub(super) max_loss_from_allocation: SchemaDec,
}

impl Limits {
    pub(super) fn of(mandate: &ValidatedMandate) -> Result<Self, SpecError> {
        let document = mandate.mandate();
        let risk = &document.risk;
        Ok(Self {
            levels: risk
                .drawdown_ladder
                .iter()
                .enumerate()
                .map(|(index, rung)| {
                    decimal(&rung.at, &format!("/risk/drawdown_ladder/{index}/at"))
                })
                .collect::<Result<Vec<_>, SpecError>>()?,
            max_daily_loss: decimal(&risk.max_daily_loss, "/risk/max_daily_loss")?,
            max_loss_from_allocation: document.capital.max_loss_from_allocation.clone(),
        })
    }

    /// Every limit's condition and whether its 1.25x hard level is reached, compared exactly (§5.2,
    /// §5.6). The profit stop is not here: it has no hard level (§3.1).
    pub(super) fn conditions(
        &self,
        figures: &Figures,
    ) -> Result<BTreeMap<LimitKey, (bool, bool)>, SpecError> {
        let hard = UsdExact::parse(HARD_TRIGGER_MULTIPLE)?;
        let equity = UsdExact::of(figures.equity);
        let high_water = UsdExact::of(figures.high_water);
        let drawdown = high_water.checked_sub(equity)?;
        let mut out = BTreeMap::new();
        for (index, level) in self.levels.iter().enumerate() {
            let soft = level.checked_mul(high_water)?;
            out.insert(
                LimitKey::DrawdownRung(rung_index(index)?),
                (
                    at_least(drawdown, soft)?,
                    at_least(drawdown, hard.checked_mul(soft)?)?,
                ),
            );
        }
        let day_start = UsdExact::of(figures.day_start);
        let daily_pnl = equity.checked_sub(day_start)?;
        let daily_loss = self.max_daily_loss.checked_mul(day_start)?;
        out.insert(
            LimitKey::MaxDailyLoss,
            (
                at_most(daily_pnl, negated(daily_loss)?)?,
                at_most(daily_pnl, negated(hard.checked_mul(daily_loss)?)?)?,
            ),
        );
        let fraction = decimal(
            &self.max_loss_from_allocation,
            "/capital/max_loss_from_allocation",
        )?;
        out.insert(
            LimitKey::LifetimeFloor,
            (
                at_most(equity, floor(figures, fraction)?)?,
                at_most(equity, floor(figures, hard.checked_mul(fraction)?)?)?,
            ),
        );
        Ok(out)
    }
}

/// C × (1 − `fraction`) + L, the lifetime floor at a loss fraction (§5.7).
pub(super) fn floor(figures: &Figures, fraction: UsdExact) -> Result<UsdExact, SpecError> {
    Ok(UsdExact::one()
        .checked_sub(fraction)?
        .checked_mul(UsdExact::of(figures.capital))?
        .checked_add(UsdExact::of(figures.inherited))?)
}

pub(super) fn at_least(value: UsdExact, level: UsdExact) -> Result<bool, SpecError> {
    Ok(!value.is_below(level)?)
}

pub(super) fn at_most(value: UsdExact, level: UsdExact) -> Result<bool, SpecError> {
    Ok(!level.is_below(value)?)
}

fn negated(value: UsdExact) -> Result<UsdExact, SpecError> {
    Ok(UsdExact::zero().checked_sub(value)?)
}

/// The five figures the limit conditions compare (§5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Figures {
    pub(super) equity: Usd,
    pub(super) high_water: Usd,
    pub(super) day_start: Usd,
    pub(super) capital: Usd,
    pub(super) inherited: Usd,
}

impl Figures {
    pub(super) fn of(snapshot: &Snapshot) -> Self {
        Self {
            equity: snapshot.agent_equity,
            high_water: snapshot.high_water_mark,
            day_start: snapshot.day_start_equity,
            capital: snapshot.capital_base,
            inherited: snapshot.inherited_loss,
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use mandate_domain::AgentMode;
    use mandate_num::{Ratio, Usd};
    use proptest::prelude::*;

    use crate::risk::{LimitKey, Snapshot, conditions};
    use crate::validate::ValidatedMandate;
    use crate::validate::tests::{context, mandate};

    /// Ten to the twelfth: every figure these tests produce has at most 12 places (§5.1's scaling),
    /// so the oracle holds each one exactly as an integer count of 10⁻¹².
    const PICO: i128 = 1_000_000_000_000;

    /// The base mandate: a 10,000 allocation, a 10% floor, a 2% daily loss, and rungs at 2% (scale
    /// by 0.5), 5% (exits only), and 8% (flatten), with a 60 s confirmation.
    pub(in crate::risk) fn base() -> Result<ValidatedMandate, String> {
        ValidatedMandate::new(mandate(&[])?, &context()?, &[]).map_err(|e| e.to_string())
    }

    /// A reported decimal as an exact count of 10⁻¹².
    fn pico(value: impl ToString) -> i128 {
        let text = value.to_string();
        let (sign, digits) = text
            .strip_prefix('-')
            .map_or((1, text.as_str()), |rest| (-1, rest));
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let padded = format!("{fraction:0<12}");
        let whole: i128 = whole.parse().unwrap_or(i128::MAX);
        let fraction: i128 = padded.parse().unwrap_or(i128::MAX);
        whole
            .saturating_mul(PICO)
            .saturating_add(fraction)
            .saturating_mul(sign)
    }

    /// §5.2's comparisons on integers, sharing nothing with the code: each rung, the daily loss, and
    /// the floor, soft and hard, at the base mandate's levels in hundred-thousandths. The figures stay
    /// below 10¹⁷ units, so no saturating operation here ever saturates.
    pub(in crate::risk) fn true_conditions(snapshot: &Snapshot) -> BTreeSet<(String, bool)> {
        let equity = pico(snapshot.agent_equity);
        let high = pico(snapshot.high_water_mark);
        let day_start = pico(snapshot.day_start_equity);
        let capital = pico(snapshot.capital_base);
        let inherited = pico(snapshot.inherited_loss);
        let times = |value: i128, by: i128| value.saturating_mul(by);
        let mut out = BTreeSet::new();
        let mut hold = |name: &str, soft: bool, hard: bool| {
            if soft {
                out.insert((name.to_owned(), false));
            }
            if hard {
                out.insert((name.to_owned(), true));
            }
        };
        let drawdown = times(high.saturating_sub(equity), 100_000);
        for (index, at) in [2_000_i128, 5_000, 8_000].into_iter().enumerate() {
            hold(
                &format!("rung {index}"),
                drawdown >= times(high, at),
                drawdown >= times(high, times(at, 5) / 4),
            );
        }
        let pnl = times(equity.saturating_sub(day_start), 100_000);
        hold(
            "daily",
            pnl <= times(day_start, -2_000),
            pnl <= times(day_start, -2_500),
        );
        let scaled_equity = times(equity, 100_000);
        let carried = times(inherited, 100_000);
        hold(
            "floor",
            scaled_equity <= times(capital, 90_000).saturating_add(carried),
            scaled_equity <= times(capital, 87_500).saturating_add(carried),
        );
        out
    }

    /// The public [`conditions`] as the oracle's set: every limit's soft and hard condition that holds.
    pub(in crate::risk) fn reported_conditions(
        mandate: &ValidatedMandate,
        snapshot: &Snapshot,
    ) -> Result<BTreeSet<(String, bool)>, String> {
        let mut out = BTreeSet::new();
        for (key, (soft, hard)) in conditions(mandate, snapshot).map_err(|e| e.to_string())? {
            let name = match key {
                LimitKey::DrawdownRung(index) => format!("rung {index}"),
                LimitKey::MaxDailyLoss => "daily".to_owned(),
                LimitKey::LifetimeFloor => "floor".to_owned(),
                LimitKey::ProfitStop => "profit".to_owned(),
            };
            if soft {
                out.insert((name.clone(), false));
            }
            if hard {
                out.insert((name, true));
            }
        }
        Ok(out)
    }

    /// A snapshot holding the five figures the conditions read, in cents, and nothing else of note.
    fn figures(cents: [i64; 5]) -> Result<Snapshot, String> {
        let dollars = |value: i64| -> Result<Usd, String> {
            let text = format!(
                "{}{}.{:02}",
                if value < 0 { "-" } else { "" },
                value.unsigned_abs() / 100,
                value.unsigned_abs() % 100
            );
            let canonical = text.trim_end_matches('0').trim_end_matches('.');
            Usd::parse(if canonical == "-" || canonical.is_empty() {
                "0"
            } else {
                canonical
            })
            .map_err(|e| e.to_string())
        };
        let [equity, high_water, day_start, capital, inherited] = cents;
        let zero = Ratio::parse("0").map_err(|e| e.to_string())?;
        Ok(Snapshot {
            agent_equity: dollars(equity)?,
            high_water_mark: dollars(high_water)?,
            drawdown: zero,
            day_start_equity: dollars(day_start)?,
            daily_pnl: Usd::ZERO,
            daily_pnl_fraction: zero,
            capital_base: dollars(capital)?,
            inherited_loss: dollars(inherited)?,
            size_factor: zero,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            restrictions: BTreeSet::new(),
            agent_mode: AgentMode::Normal,
            instrument_restrictions: BTreeSet::new(),
            net_contributed: Usd::ZERO,
        })
    }

    /// Figures that sit exactly on one of the base mandate's levels or a cent either side of it: a
    /// rung's fall of 2%, 5%, or 8% of the high-water mark or 1.25 times one; a day's fall of 2% or
    /// 2.5% of its start; or the floor at 90% or 87.5% of capital plus the inherited loss. The other
    /// figures are drawn freely, so every condition is also seen far from its boundary.
    fn figure_draws() -> impl Strategy<Value = [i64; 5]> {
        let nudge = prop::sample::select(vec![-1_i64, 0, 1]);
        let free = 0_i64..=150_000;
        prop_oneof![
            (
                80_i64..=120,
                prop::sample::select(vec![200_i64, 250, 500, 625, 800, 1_000]),
                nudge.clone(),
                free.clone(),
                900_000_i64..=1_100_000,
            )
                .prop_map(|(high, fall, nudge, above, capital)| {
                    let high_water = high.saturating_mul(10_000);
                    let equity = high_water
                        .saturating_sub(high.saturating_mul(fall))
                        .saturating_add(nudge);
                    [equity, high_water, equity.saturating_add(above), capital, 0]
                }),
            (
                80_i64..=120,
                prop::sample::select(vec![200_i64, 250]),
                nudge.clone(),
                free.clone(),
                900_000_i64..=1_100_000,
            )
                .prop_map(|(start, fall, nudge, below, capital)| {
                    let day_start = start.saturating_mul(10_000);
                    let equity = day_start
                        .saturating_sub(start.saturating_mul(fall))
                        .saturating_add(nudge);
                    [equity, equity.saturating_add(below), day_start, capital, 0]
                }),
            (
                90_i64..=110,
                prop::sample::select(vec![9_000_i64, 8_750]),
                prop::sample::select(vec![0_i64, 10_000, 50_000]),
                nudge,
                free,
            )
                .prop_map(|(capital, kept, inherited, nudge, below)| {
                    let equity = capital
                        .saturating_mul(kept)
                        .saturating_add(inherited)
                        .saturating_add(nudge);
                    [
                        equity,
                        equity.saturating_add(below),
                        equity,
                        capital.saturating_mul(10_000),
                        inherited,
                    ]
                }),
        ]
    }

    proptest! {
        /// §5.2's comparisons, soft and hard (§5.6), as the public [`conditions`] makes them, are the
        /// integer oracle's for any figures, boundaries included: a fall of exactly 2%, 5%, or 8% of
        /// the high-water mark, or of 2% or 2.5% of the day's start, is drawn often.
        #[test]
        fn the_conditions_are_the_integer_readings_at_any_figures(cents in figure_draws()) {
            let mandate = base().map_err(TestCaseError::fail)?;
            let snapshot = figures(cents).map_err(TestCaseError::fail)?;
            prop_assert_eq!(
                reported_conditions(&mandate, &snapshot).map_err(TestCaseError::fail)?,
                true_conditions(&snapshot)
            );
        }
    }

    /// Every condition comes true, soft and hard, and none is true at the high-water mark: a 12% fall
    /// on the day from a 10,000 high, with capital at 11,000 so the floor's 9,900 lies above it.
    #[test]
    fn each_condition_holds_past_its_level_and_none_at_the_high_water_mark() -> Result<(), String> {
        let mandate = base()?;
        let all: BTreeSet<(String, bool)> = ["rung 0", "rung 1", "rung 2", "daily", "floor"]
            .into_iter()
            .flat_map(|name| [(name.to_owned(), false), (name.to_owned(), true)])
            .collect();
        let fallen = figures([880_000, 1_000_000, 1_000_000, 1_100_000, 0])?;
        assert_eq!(reported_conditions(&mandate, &fallen)?, all);
        assert_eq!(true_conditions(&fallen), all, "the oracle agrees");
        let flat = figures([1_000_000, 1_000_000, 1_000_000, 1_000_000, 0])?;
        assert_eq!(reported_conditions(&mandate, &flat)?, BTreeSet::new());
        Ok(())
    }
}
