//! The comparisons of §5.2 and §5.6: each rung, the daily loss, and the lifetime floor, soft and at
//! the 1.25x hard level, and each rung's lift level, compared exactly on the figures a state reports.
//!
//! They read only the mandate's levels and five figures, so the public [`conditions`](super::conditions)
//! and the fold behind [`RiskState`](super::RiskState) make the same comparisons by construction.

use std::collections::BTreeMap;

use mandate_num::{Usd, UsdExact};

use super::{HARD_TRIGGER_MULTIPLE, LimitKey, Snapshot, decimal, rung_index};
use crate::document::Goal;
use crate::validate::ValidatedMandate;
use crate::{SchemaDec, SpecError};

/// The levels the conditions compare against (§5.4, §5.5, §5.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Limits {
    /// Each rung's `at`, in ladder order.
    pub(super) levels: Vec<UsdExact>,
    hysteresis: UsdExact,
    max_daily_loss: UsdExact,
    /// The one level a version can change while a state lives: only a loosening version, which is
    /// the one path that lifts the floor (§5.7). It stays the mandate's text, not a `UsdExact`,
    /// because §5.7's loosening check compares it with the new fraction, which arrives as a
    /// [`SchemaDec`], and [`ApplyResult`](super::ApplyResult) journals that fraction as written.
    pub(super) max_loss_from_allocation: SchemaDec,
    /// A `profit_stop` goal's `profit_level`, and nothing for any other goal (§3.1).
    profit_level: Option<UsdExact>,
}

/// Every §5.2 comparison at one state, so the fold reads each from here and nowhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Readings {
    /// Each limit's condition and whether its 1.25x hard level is reached (§5.6).
    pub(super) limits: BTreeMap<LimitKey, (bool, bool)>,
    /// A `profit_stop` goal's condition, E − C ≥ `profit_level` × C, which has no hard level (§3.1,
    /// §5.6); nothing for any other goal.
    pub(super) profit: Option<bool>,
    /// By rung index, one entry for every rung: H − E < (`at` − `hysteresis`) × H, the drawdown back
    /// past the rung's lift level (§5.5).
    ///
    /// Only a `scale_sizes` rung lifts on it (§5.5). A latched `exits_only` or `flatten_and_pause`
    /// rung is read here too, but lifts only on the owner's acknowledgment (§5.8), whatever this says.
    pub(super) below_lift: BTreeMap<u8, bool>,
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
            hysteresis: decimal(&risk.hysteresis, "/risk/hysteresis")?,
            max_daily_loss: decimal(&risk.max_daily_loss, "/risk/max_daily_loss")?,
            max_loss_from_allocation: document.capital.max_loss_from_allocation.clone(),
            profit_level: match &document.goal {
                Goal::ProfitStop { profit_level, .. } => {
                    Some(decimal(profit_level, "/goal/profit_level")?)
                }
                Goal::Continuous { .. } | Goal::Accumulate { .. } => None,
            },
        })
    }

    /// The daily loss's condition, soft and at the 1.25x hard level, against a day-start equity:
    /// E − E₀ ≤ −`max_daily_loss` × E₀ (§5.4, §5.6). The fold also reads a breach carried over the
    /// rollover against the previous day's E₀ with it.
    pub(super) fn daily_loss(
        &self,
        equity: Usd,
        day_start: Usd,
    ) -> Result<(bool, bool), SpecError> {
        let hard = UsdExact::parse(HARD_TRIGGER_MULTIPLE)?;
        let day_start = UsdExact::of(day_start);
        let daily_pnl = UsdExact::of(equity).checked_sub(day_start)?;
        let daily_loss = self.max_daily_loss.checked_mul(day_start)?;
        Ok((
            at_most(daily_pnl, negated(daily_loss)?)?,
            at_most(daily_pnl, negated(hard.checked_mul(daily_loss)?)?)?,
        ))
    }

    /// Every limit's condition, soft and at the 1.25x hard level, each rung's lift, and a
    /// `profit_stop` goal's condition, all as exact products (§5.2, §5.6). The profit stop is kept out
    /// of `limits`: it is a goal's stop condition with no hard level (§3.1), which the risk state
    /// confirms.
    pub(super) fn conditions(&self, figures: &Figures) -> Result<Readings, SpecError> {
        let hard = UsdExact::parse(HARD_TRIGGER_MULTIPLE)?;
        let equity = UsdExact::of(figures.equity);
        let high_water = UsdExact::of(figures.high_water);
        let drawdown = high_water.checked_sub(equity)?;
        let mut limits = BTreeMap::new();
        let mut below_lift = BTreeMap::new();
        for (index, level) in self.levels.iter().enumerate() {
            let rung = rung_index(index)?;
            let soft = level.checked_mul(high_water)?;
            limits.insert(
                LimitKey::DrawdownRung(rung),
                (
                    at_least(drawdown, soft)?,
                    at_least(drawdown, hard.checked_mul(soft)?)?,
                ),
            );
            let lift = level
                .checked_sub(self.hysteresis)?
                .checked_mul(high_water)?;
            below_lift.insert(rung, drawdown.is_below(lift)?);
        }
        limits.insert(
            LimitKey::MaxDailyLoss,
            self.daily_loss(figures.equity, figures.day_start)?,
        );
        let fraction = decimal(
            &self.max_loss_from_allocation,
            "/capital/max_loss_from_allocation",
        )?;
        limits.insert(
            LimitKey::LifetimeFloor,
            (
                at_most(equity, floor(figures, fraction)?)?,
                at_most(equity, floor(figures, hard.checked_mul(fraction)?)?)?,
            ),
        );
        let capital = UsdExact::of(figures.capital);
        let profit = self
            .profit_level
            .map(|level| at_least(equity.checked_sub(capital)?, level.checked_mul(capital)?))
            .transpose()?;
        Ok(Readings {
            limits,
            below_lift,
            profit,
        })
    }
}

/// C × (1 − `fraction`) + L, the lifetime floor at a loss fraction (§5.7).
fn floor(figures: &Figures, fraction: UsdExact) -> Result<UsdExact, SpecError> {
    Ok(UsdExact::one()
        .checked_sub(fraction)?
        .checked_mul(UsdExact::of(figures.capital))?
        .checked_add(UsdExact::of(figures.inherited))?)
}

fn at_least(value: UsdExact, level: UsdExact) -> Result<bool, SpecError> {
    Ok(!value.is_below(level)?)
}

fn at_most(value: UsdExact, level: UsdExact) -> Result<bool, SpecError> {
    Ok(!level.is_below(value)?)
}

fn negated(value: UsdExact) -> Result<UsdExact, SpecError> {
    Ok(UsdExact::zero().checked_sub(value)?)
}

/// The five figures the limit conditions compare (§5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Figures {
    pub(super) equity: Usd,
    /// H, the peak of a positive equity (§5.5). The fold opens it at the allocation, which the schema
    /// makes positive, and only raises it. A negative H would put each rung's 1.25x hard level below
    /// its soft one.
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
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    use super::{Figures, Limits};
    use crate::risk::{LimitKey, Snapshot, conditions};
    use crate::validate::ValidatedMandate;
    use crate::validate::tests::{context, mandate};

    /// The places the oracle holds a figure at. The draws give a figure 12 to 21, the most an equity
    /// carries (a 9-place quantity at a 12-place mark), so a level times a figure needs up to 30.
    const FIGURE_PLACES: u32 = 21;
    /// The places the oracle holds a level at. The draws give a level 1 to 9.
    const LEVEL_PLACES: u32 = 9;
    /// One, counted in the oracle's level units.
    const LEVEL_ONE: i128 = 1_000_000_000;
    /// The largest magnitude a drawn figure has, 10⁴ in 10⁻²¹, so every product the oracle forms
    /// fits `i128`.
    const FIGURE_COUNT_BOUND: i128 = 10_000_000_000_000_000_000_000_000;

    /// The base mandate: a 10,000 allocation, a 10% floor, a 2% daily loss, and rungs at 2% (scale
    /// by 0.5), 5% (exits only), and 8% (flatten), with a 1% hysteresis and a 60 s confirmation.
    pub(in crate::risk) fn base() -> Result<ValidatedMandate, String> {
        Levels::base().mandate()
    }

    /// Which of a limit's comparisons holds.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub(in crate::risk) enum Reading {
        Soft,
        Hard,
        BelowLift,
    }

    /// The levels the conditions read, as the text written into the mandate: the three rungs' `at`
    /// (the last is `max_drawdown`), `hysteresis`, `max_daily_loss`, and `max_loss_from_allocation`.
    #[derive(Debug, Clone)]
    pub(in crate::risk) struct Levels {
        rungs: [String; 3],
        hysteresis: String,
        max_daily_loss: String,
        max_loss_from_allocation: String,
    }

    impl Levels {
        fn base() -> Self {
            Self {
                rungs: ["0.02".to_owned(), "0.05".to_owned(), "0.08".to_owned()],
                hysteresis: "0.01".to_owned(),
                max_daily_loss: "0.02".to_owned(),
                max_loss_from_allocation: "0.1".to_owned(),
            }
        }

        fn mandate(&self) -> Result<ValidatedMandate, String> {
            let quoted = |text: &str| format!("\"{text}\"");
            let patches = [
                ("/risk/drawdown_ladder/0/at", quoted(&self.rungs[0])),
                ("/risk/drawdown_ladder/1/at", quoted(&self.rungs[1])),
                ("/risk/drawdown_ladder/2/at", quoted(&self.rungs[2])),
                ("/risk/max_drawdown", quoted(&self.rungs[2])),
                ("/risk/hysteresis", quoted(&self.hysteresis)),
                ("/risk/max_daily_loss", quoted(&self.max_daily_loss)),
                (
                    "/capital/max_loss_from_allocation",
                    quoted(&self.max_loss_from_allocation),
                ),
            ];
            let borrowed: Vec<(&str, &str)> = patches
                .iter()
                .map(|(path, value)| (*path, value.as_str()))
                .collect();
            ValidatedMandate::new(mandate(&borrowed)?, &context()?, &[])
                .map_err(|e| format!("{self:?}: {e}"))
        }
    }

    /// An oracle step that does not fit `i128` fails the test, so the oracle never agrees by
    /// saturating.
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

    fn ten(places: u32) -> Result<i128, String> {
        fits(10_i128.checked_pow(places))
    }

    fn fits_places(places: Option<u32>) -> Result<u32, String> {
        places.ok_or_else(|| "a place count went below zero".to_owned())
    }

    fn rem(left: i128, right: i128) -> Result<i128, String> {
        fits(left.checked_rem(right))
    }

    /// A decimal as an exact count of 10^-`places`. Text with more places than that, text that is not
    /// plain decimal, and a value `i128` cannot hold are refused, never read at another scale.
    fn units(value: &str, places: u32) -> Result<i128, String> {
        let (negative, digits) = value
            .strip_prefix('-')
            .map_or((false, value), |rest| (true, rest));
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let plain = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if whole.is_empty() || !plain(whole) || !plain(fraction) {
            return Err(format!("`{value}` is not plain decimal text"));
        }
        let width = usize::try_from(places).map_err(|e| e.to_string())?;
        if fraction.len() > width {
            return Err(format!("`{value}` has more than {places} places"));
        }
        let whole: i128 = whole.parse().map_err(|e| format!("`{value}`: {e}"))?;
        let fraction: i128 = format!("{fraction:0<width$}")
            .parse()
            .map_err(|e| format!("`{value}`: {e}"))?;
        let magnitude = add(mul(whole, ten(places)?)?, fraction)?;
        if negative {
            sub(0, magnitude)
        } else {
            Ok(magnitude)
        }
    }

    /// The canonical text of `count` × 10^-`places`: no trailing zero, and no point without a fraction.
    fn text(count: i128, places: u32) -> Result<String, String> {
        let scale = ten(places)?.unsigned_abs();
        let magnitude = count.unsigned_abs();
        let width = usize::try_from(places).map_err(|e| e.to_string())?;
        let overflow = || "the oracle's arithmetic overflowed u128".to_owned();
        let fraction = format!(
            "{:0width$}",
            magnitude.checked_rem(scale).ok_or_else(overflow)?
        );
        let fraction = fraction.trim_end_matches('0');
        let sign = if count < 0 { "-" } else { "" };
        let whole = magnitude.checked_div(scale).ok_or_else(overflow)?;
        Ok(if fraction.is_empty() {
            format!("{sign}{whole}")
        } else {
            format!("{sign}{whole}.{fraction}")
        })
    }

    /// §5.2's comparisons on integers, sharing nothing with the code: figures in 10⁻²¹ and levels in
    /// 10⁻⁹, so each product is an exact count of 10⁻³⁰, and a 1.25x hard level is 5 times a product
    /// against 4 times the other side. The levels are read from the text the draws wrote, not from
    /// the mandate the code parsed.
    pub(in crate::risk) fn true_readings(
        levels: &Levels,
        snapshot: &Snapshot,
    ) -> Result<BTreeSet<(String, Reading)>, String> {
        let figure = |value: Usd| units(&value.to_string(), FIGURE_PLACES);
        let level = |value: &str| units(value, LEVEL_PLACES);
        let equity = figure(snapshot.agent_equity)?;
        let high = figure(snapshot.high_water_mark)?;
        let day_start = figure(snapshot.day_start_equity)?;
        let capital = figure(snapshot.capital_base)?;
        let inherited = figure(snapshot.inherited_loss)?;
        let hysteresis = level(&levels.hysteresis)?;
        let mut out = BTreeSet::new();
        let drawdown = mul(sub(high, equity)?, LEVEL_ONE)?;
        for (index, at) in levels.rungs.iter().enumerate() {
            let at = level(at)?;
            let soft = mul(at, high)?;
            let lift = mul(sub(at, hysteresis)?, high)?;
            for (reading, holds) in [
                (Reading::Soft, drawdown >= soft),
                (Reading::Hard, mul(drawdown, 4)? >= mul(soft, 5)?),
                (Reading::BelowLift, drawdown < lift),
            ] {
                if holds {
                    out.insert((format!("rung {index}"), reading));
                }
            }
        }
        let pnl = mul(sub(equity, day_start)?, LEVEL_ONE)?;
        let loss = sub(0, mul(level(&levels.max_daily_loss)?, day_start)?)?;
        let fraction = level(&levels.max_loss_from_allocation)?;
        let scaled = mul(equity, LEVEL_ONE)?;
        let carried = mul(inherited, LEVEL_ONE)?;
        let soft_floor = add(mul(capital, sub(LEVEL_ONE, fraction)?)?, carried)?;
        let hard_floor = add(
            mul(capital, sub(mul(LEVEL_ONE, 4)?, mul(fraction, 5)?)?)?,
            mul(carried, 4)?,
        )?;
        for (name, reading, holds) in [
            ("daily", Reading::Soft, pnl <= loss),
            ("daily", Reading::Hard, mul(pnl, 4)? <= mul(loss, 5)?),
            ("floor", Reading::Soft, scaled <= soft_floor),
            ("floor", Reading::Hard, mul(scaled, 4)? <= hard_floor),
        ] {
            if holds {
                out.insert((name.to_owned(), reading));
            }
        }
        Ok(out)
    }

    fn name(key: LimitKey) -> String {
        match key {
            LimitKey::DrawdownRung(index) => format!("rung {index}"),
            LimitKey::MaxDailyLoss => "daily".to_owned(),
            LimitKey::LifetimeFloor => "floor".to_owned(),
            LimitKey::ProfitStop => "profit".to_owned(),
        }
    }

    /// The code's readings as the oracle's set: the limits from the public [`conditions`], which
    /// must be the ones the fold reads, and the lifts from [`Limits::conditions`].
    pub(in crate::risk) fn reported_readings(
        mandate: &ValidatedMandate,
        snapshot: &Snapshot,
    ) -> Result<BTreeSet<(String, Reading)>, String> {
        let readings = Limits::of(mandate)
            .and_then(|limits| limits.conditions(&Figures::of(snapshot)))
            .map_err(|e| e.to_string())?;
        let public = conditions(mandate, snapshot).map_err(|e| e.to_string())?;
        if public != readings.limits {
            return Err(format!(
                "the public conditions {public:?} are not the fold's {:?}",
                readings.limits
            ));
        }
        let mut out = BTreeSet::new();
        for (key, (soft, hard)) in public {
            for (reading, holds) in [(Reading::Soft, soft), (Reading::Hard, hard)] {
                if holds {
                    out.insert((name(key), reading));
                }
            }
        }
        for (index, below) in readings.below_lift {
            if below {
                out.insert((format!("rung {index}"), Reading::BelowLift));
            }
        }
        Ok(out)
    }

    /// A snapshot holding the five figures the conditions read, as decimal text, and nothing else of
    /// note: equity, high-water mark, day-start equity, capital base, and inherited loss.
    fn snapshot(figures: &[String; 5]) -> Result<Snapshot, String> {
        let usd = |value: &str| Usd::parse(value).map_err(|e| format!("`{value}`: {e}"));
        let [equity, high_water, day_start, capital, inherited] = figures;
        let zero = Ratio::parse("0").map_err(|e| e.to_string())?;
        Ok(Snapshot {
            agent_equity: usd(equity)?,
            high_water_mark: usd(high_water)?,
            drawdown: zero,
            day_start_equity: usd(day_start)?,
            daily_pnl: Usd::ZERO,
            daily_pnl_fraction: zero,
            capital_base: usd(capital)?,
            inherited_loss: usd(inherited)?,
            size_factor: zero,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            restrictions: BTreeSet::new(),
            agent_mode: AgentMode::Normal,
            instrument_restrictions: BTreeSet::new(),
            net_contributed: Usd::ZERO,
            tripwire_restriction: false,
        })
    }

    /// `count` of 10⁻⁹ cut down to `places` places, or kept whole when the cut would leave nothing.
    fn coarse(count: i128, places: u32) -> Result<i128, String> {
        let step = ten(fits_places(LEVEL_PLACES.checked_sub(places))?)?;
        let kept = mul(fits(count.checked_div(step))?, step)?;
        Ok(if kept == 0 { count } else { kept })
    }

    /// A valid ladder and its neighbours, each level with 1 to 9 places: three strictly increasing
    /// rungs up to the platform's 0.5 floor bound, the hysteresis below the first, the floor's
    /// fraction at or above the last, and any daily loss.
    fn level_draws() -> impl Strategy<Value = Result<Levels, String>> {
        let places = || 1_u32..=9;
        (
            (2_i128..=200_000_000, places()),
            (1_i128..=150_000_000, places()),
            (1_i128..=150_000_000, places()),
            (0_i128..LEVEL_ONE, places()),
            (1_i128..LEVEL_ONE, places()),
            (0_i128..LEVEL_ONE, places()),
        )
            .prop_map(|(first, gap, second_gap, hysteresis, daily, floor)| {
                let first_at = coarse(first.0, first.1)?;
                let second_at = add(first_at, coarse(gap.0, gap.1)?)?;
                let last_at = add(second_at, coarse(second_gap.0, second_gap.1)?)?;
                let hysteresis =
                    coarse(add(1, rem(hysteresis.0, sub(first_at, 1)?)?)?, hysteresis.1)?;
                let headroom = add(sub(LEVEL_ONE / 2, last_at)?, 1)?;
                let fraction = add(last_at, rem(floor.0, headroom)?)?;
                let coarse_fraction = coarse(fraction, floor.1)?;
                let at = |count: i128| text(count, LEVEL_PLACES);
                Ok(Levels {
                    rungs: [at(first_at)?, at(second_at)?, at(last_at)?],
                    hysteresis: at(hysteresis)?,
                    max_daily_loss: at(coarse(daily.0, daily.1)?)?,
                    max_loss_from_allocation: at(if coarse_fraction < last_at {
                        fraction
                    } else {
                        coarse_fraction
                    })?,
                })
            })
    }

    /// A figure with at most 12 to 21 places, its whole part within 10⁴ in magnitude, as a count
    /// of 10⁻²¹.
    fn figure_draw() -> impl Strategy<Value = Result<i128, String>> {
        (12_u32..=21, -FIGURE_COUNT_BOUND..=FIGURE_COUNT_BOUND).prop_map(|(places, count)| {
            let step = ten(fits_places(FIGURE_PLACES.checked_sub(places))?)?;
            mul(fits(count.checked_div(step))?, step)
        })
    }

    /// The comparison a case sits on.
    #[derive(Debug, Clone, Copy)]
    enum Target {
        Rung(usize, Reading),
        Daily(Reading),
        Floor(Reading),
        Free,
    }

    fn target_draws() -> impl Strategy<Value = Target> {
        let reading = prop::sample::select(vec![Reading::Soft, Reading::Hard, Reading::BelowLift]);
        let side = prop::sample::select(vec![Reading::Soft, Reading::Hard]);
        prop_oneof![
            3 => (0_usize..3, reading).prop_map(|(index, reading)| Target::Rung(index, reading)),
            1 => side.clone().prop_map(Target::Daily),
            1 => side.prop_map(Target::Floor),
            1 => Just(Target::Free),
        ]
    }

    /// `numerator ÷ denominator` rounded toward negative infinity.
    fn floor_div(numerator: i128, denominator: i128) -> Result<i128, String> {
        fits(numerator.checked_div_euclid(denominator))
    }

    /// The five figures with equity moved to within one 10⁻²¹ of `target`'s level, on it where the
    /// level is a 21-place figure, and on either side where it is not. The high-water mark is kept
    /// non-negative, as a peak of equity is; the other figures keep their drawn sign.
    fn placed(
        levels: &Levels,
        target: Target,
        nudge: i128,
        drawn: [i128; 5],
    ) -> Result<[String; 5], String> {
        let [mut equity, high, day_start, capital, inherited] = drawn;
        let high = high.abs();
        let level = |value: &str| units(value, LEVEL_PLACES);
        let hard_scaled = |product: i128| floor_div(mul(product, 5)?, mul(LEVEL_ONE, 4)?);
        match target {
            Target::Rung(index, reading) => {
                let at = level(levels.rungs.get(index).ok_or("no such rung")?)?;
                let fall = match reading {
                    Reading::Soft => floor_div(mul(at, high)?, LEVEL_ONE)?,
                    Reading::Hard => hard_scaled(mul(at, high)?)?,
                    Reading::BelowLift => {
                        floor_div(mul(sub(at, level(&levels.hysteresis)?)?, high)?, LEVEL_ONE)?
                    }
                };
                equity = add(sub(high, fall)?, nudge)?;
            }
            Target::Daily(reading) => {
                let loss = mul(level(&levels.max_daily_loss)?, day_start)?;
                let fall = match reading {
                    Reading::Hard => hard_scaled(loss)?,
                    Reading::Soft | Reading::BelowLift => floor_div(loss, LEVEL_ONE)?,
                };
                equity = add(sub(day_start, fall)?, nudge)?;
            }
            Target::Floor(reading) => {
                let fraction = level(&levels.max_loss_from_allocation)?;
                let carried = mul(inherited, LEVEL_ONE)?;
                let floor = match reading {
                    Reading::Hard => floor_div(
                        add(
                            mul(capital, sub(mul(LEVEL_ONE, 4)?, mul(fraction, 5)?)?)?,
                            mul(carried, 4)?,
                        )?,
                        mul(LEVEL_ONE, 4)?,
                    )?,
                    Reading::Soft | Reading::BelowLift => floor_div(
                        add(mul(capital, sub(LEVEL_ONE, fraction)?)?, carried)?,
                        LEVEL_ONE,
                    )?,
                };
                equity = add(floor, nudge)?;
            }
            Target::Free => {}
        }
        let as_text = |count: i128| text(count, FIGURE_PLACES);
        Ok([
            as_text(equity)?,
            as_text(high)?,
            as_text(day_start)?,
            as_text(capital)?,
            as_text(inherited)?,
        ])
    }

    /// One case: drawn levels, and figures that sit on one of their comparisons or away from all.
    fn case_draws() -> impl Strategy<Value = Result<(Levels, [String; 5]), String>> {
        (
            level_draws(),
            target_draws(),
            -1_i128..=1,
            prop::array::uniform5(figure_draw()),
        )
            .prop_map(|(levels, target, nudge, drawn)| {
                let levels = levels?;
                let [a, b, c, d, e] = drawn;
                let figures = placed(&levels, target, nudge, [a?, b?, c?, d?, e?])?;
                Ok((levels, figures))
            })
    }

    proptest! {
        /// §5.2's comparisons, soft and hard (§5.6), and each rung's lift, as the code makes them, are
        /// the integer oracle's at any levels and figures. The figures carry 12 to 21 places and the
        /// levels up to 9, so the products need up to 30, and a case often sits within 10⁻²¹ of one
        /// comparison's level: any product rounded before it is compared moves a verdict.
        #[test]
        fn the_conditions_are_the_integer_readings_at_any_levels_and_figures(case in case_draws()) {
            let (levels, figures) = case.map_err(TestCaseError::fail)?;
            let mandate = levels.mandate().map_err(TestCaseError::fail)?;
            let snapshot = snapshot(&figures).map_err(TestCaseError::fail)?;
            prop_assert_eq!(
                reported_readings(&mandate, &snapshot).map_err(TestCaseError::fail)?,
                true_readings(&levels, &snapshot).map_err(TestCaseError::fail)?,
                "levels {:?}, figures {:?}", levels, figures
            );
        }
    }

    /// The draws reach what the property claims to test: over 512 cases, rounding each rung's, the
    /// daily loss's, and the floor's product to 12 places before comparing changes some verdict in
    /// each family. Whole-cent figures at round levels, the draws #289's round 1 found, change none.
    #[test]
    fn the_draws_reach_products_a_twelve_place_rounding_would_misjudge() -> Result<(), String> {
        let twelve = ten(30 - 12)?;
        let rounded = |product: i128| -> Result<i128, String> {
            mul(floor_div(add(product, twelve / 2)?, twelve)?, twelve)
        };
        let mut runner = TestRunner::deterministic();
        let mut changed: BTreeSet<&str> = BTreeSet::new();
        for _ in 0..512 {
            let (levels, figures) = case_draws()
                .new_tree(&mut runner)
                .map_err(|e| e.to_string())?
                .current()?;
            let snapshot = snapshot(&figures)?;
            let figure = |value: Usd| units(&value.to_string(), FIGURE_PLACES);
            let level = |value: &str| units(value, LEVEL_PLACES);
            let equity = figure(snapshot.agent_equity)?;
            let high = figure(snapshot.high_water_mark)?;
            let day_start = figure(snapshot.day_start_equity)?;
            let drawdown = mul(sub(high, equity)?, LEVEL_ONE)?;
            for at in &levels.rungs {
                let product = mul(level(at)?, high)?;
                if (drawdown >= product) != (drawdown >= rounded(product)?) {
                    changed.insert("rung");
                }
            }
            let pnl = mul(sub(equity, day_start)?, LEVEL_ONE)?;
            let loss = mul(level(&levels.max_daily_loss)?, day_start)?;
            if (pnl <= -loss) != (pnl <= -rounded(loss)?) {
                changed.insert("daily");
            }
            let kept = mul(
                figure(snapshot.capital_base)?,
                sub(LEVEL_ONE, level(&levels.max_loss_from_allocation)?)?,
            )?;
            let carried = mul(figure(snapshot.inherited_loss)?, LEVEL_ONE)?;
            let scaled = mul(equity, LEVEL_ONE)?;
            if (scaled <= add(kept, carried)?) != (scaled <= add(rounded(kept)?, carried)?) {
                changed.insert("floor");
            }
        }
        assert_eq!(
            changed,
            BTreeSet::from(["daily", "floor", "rung"]),
            "every family has a case a 12-place rounding would misjudge"
        );
        Ok(())
    }

    /// The oracle refuses what it cannot hold rather than reading it at another scale: a 22-place
    /// figure, a 10-place level, an exponent, a stray sign, and a value past `i128`.
    #[test]
    fn the_oracle_refuses_a_figure_it_cannot_hold() {
        for (value, places) in [
            ("0.0000000000000000000001", FIGURE_PLACES),
            ("0.0000000001", LEVEL_PLACES),
            ("1e5", FIGURE_PLACES),
            ("--1", FIGURE_PLACES),
            ("", FIGURE_PLACES),
            ("200000000000000000", FIGURE_PLACES),
        ] {
            assert!(
                units(value, places).is_err(),
                "`{value}` at {places} places is refused"
            );
        }
        assert_eq!(units("-0.000000000000000000001", FIGURE_PLACES), Ok(-1));
        assert_eq!(units("12.5", LEVEL_PLACES), Ok(12_500_000_000));
    }

    /// Every condition comes true, soft and hard, and none is true at the high-water mark, where every
    /// rung is below its lift level: a 12% fall on the day from a 10,000 high, with capital at 11,000
    /// so the floor's 9,900 lies above it.
    #[test]
    fn each_condition_holds_past_its_level_and_none_at_the_high_water_mark() -> Result<(), String> {
        let levels = Levels::base();
        let mandate = base()?;
        let figures = |values: [&str; 5]| snapshot(&values.map(str::to_owned));
        let all: BTreeSet<(String, Reading)> = ["rung 0", "rung 1", "rung 2", "daily", "floor"]
            .into_iter()
            .flat_map(|name| {
                [
                    (name.to_owned(), Reading::Soft),
                    (name.to_owned(), Reading::Hard),
                ]
            })
            .collect();
        let fallen = figures(["8800", "10000", "10000", "11000", "0"])?;
        assert_eq!(reported_readings(&mandate, &fallen)?, all);
        assert_eq!(true_readings(&levels, &fallen)?, all, "the oracle agrees");
        let lifted: BTreeSet<(String, Reading)> = ["rung 0", "rung 1", "rung 2"]
            .into_iter()
            .map(|name| (name.to_owned(), Reading::BelowLift))
            .collect();
        let flat = figures(["10000", "10000", "10000", "10000", "0"])?;
        assert_eq!(reported_readings(&mandate, &flat)?, lifted);
        assert_eq!(true_readings(&levels, &flat)?, lifted, "the oracle agrees");
        Ok(())
    }

    /// The conditions read every limit whether or not it holds, and nothing else: the three rungs,
    /// the daily loss, and the floor, never the profit stop (§3.1), and a lift reading for every rung
    /// index. Checked at a state where nothing holds and at one where everything does, so an entry
    /// left out when its readings are false, or one added as `(false, false)`, fails.
    #[test]
    fn the_conditions_read_exactly_every_rung_the_daily_loss_and_the_floor() -> Result<(), String> {
        let limits = Limits::of(&base()?).map_err(|e| e.to_string())?;
        let keys = BTreeSet::from([
            LimitKey::DrawdownRung(0),
            LimitKey::DrawdownRung(1),
            LimitKey::DrawdownRung(2),
            LimitKey::MaxDailyLoss,
            LimitKey::LifetimeFloor,
        ]);
        let rungs = BTreeSet::from([0_u8, 1, 2]);
        for values in [
            ["10000", "10000", "10000", "10000", "0"],
            ["8800", "10000", "10000", "11000", "0"],
        ] {
            let state = snapshot(&values.map(str::to_owned))?;
            let readings = limits
                .conditions(&Figures::of(&state))
                .map_err(|e| e.to_string())?;
            assert_eq!(
                readings.limits.keys().copied().collect::<BTreeSet<_>>(),
                keys,
                "the limits read at {values:?}"
            );
            assert_eq!(
                readings.below_lift.keys().copied().collect::<BTreeSet<_>>(),
                rungs,
                "the lifts read at {values:?}"
            );
        }
        Ok(())
    }

    /// A `profit_stop` goal is read as E − C ≥ `profit_level` × C, exactly (§3.1): with C at 10,000
    /// and a level of 0.1, equity on 11,000 holds it and one 10⁻²¹ below does not. A mandate with
    /// any other goal has no profit reading at all.
    #[test]
    fn the_profit_stop_is_read_on_its_level_and_only_for_its_goal() -> Result<(), String> {
        let goal = "{\"type\": \"profit_stop\", \"profit_level\": \"0.1\", \"end_date\": null}";
        let stop = ValidatedMandate::new(mandate(&[("/goal", goal)])?, &context()?, &[])
            .map_err(|e| e.to_string())?;
        let read = |mandate: &ValidatedMandate, equity: &str| -> Result<Option<bool>, String> {
            let state = snapshot(&[equity, "20000", "10000", "10000", "0"].map(str::to_owned))?;
            Ok(Limits::of(mandate)
                .and_then(|limits| limits.conditions(&Figures::of(&state)))
                .map_err(|e| e.to_string())?
                .profit)
        };
        assert_eq!(read(&stop, "11000")?, Some(true), "on the level");
        assert_eq!(
            read(&stop, "10999.999999999999999999999")?,
            Some(false),
            "one 10⁻²¹ below it"
        );
        assert_eq!(read(&base()?, "20000")?, None, "no profit_stop goal");
        Ok(())
    }

    /// A rung lifts only strictly inside its lift level (§5.2): at the base mandate's first rung, 2%
    /// with a 1% hysteresis, a fall of exactly 1% of a 10,000 high is not below the level, and one
    /// 10⁻²¹ less is.
    #[test]
    fn a_rung_is_below_its_lift_level_only_strictly_inside_it() -> Result<(), String> {
        let mandate = base()?;
        let below = |equity: &str| -> Result<bool, String> {
            let state = snapshot(&[equity, "10000", "10000", "10000", "0"].map(str::to_owned))?;
            Ok(reported_readings(&mandate, &state)?
                .contains(&("rung 0".to_owned(), Reading::BelowLift)))
        };
        assert!(!below("9900")?, "on the lift level is not below it");
        assert!(
            below("9900.000000000000000000001")?,
            "one 10⁻²¹ inside is below it"
        );
        assert!(
            !below("9899.999999999999999999999")?,
            "one 10⁻²¹ outside is not"
        );
        Ok(())
    }
}
