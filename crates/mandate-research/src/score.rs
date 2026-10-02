//! E17-8's forward-paper evaluator, the pure scoring half (DEC-99, DEC-122, DEC-281): a closed
//! thesis, a close series, the pre-registered decision, and a modeled round-trip cost in; a
//! scorecard out. The evaluator scores **ideas, not executions** — the input carries no admission
//! outcome and no predecessor's score, so a refused thesis and an admitted one score the same
//! (DEC-99's "every thesis") and a revision starts from zero by construction (MI-18), its row
//! showing the lineage's revision count.
//!
//! Every figure is an exact decimal or one named rounding of one formula, the E4-2 report
//! discipline: no clock, no randomness, no floats (ES-21). The evaluator never reads an envelope
//! field (MI-16) and never journals: the shell selects the window's closed theses from the
//! journal, fetches the close series, and computes each thesis's round-trip cost from the same
//! §6.4 fee model the backtest charges — all three arrive typed, so the crate depends on no
//! market-data crate (layer 6 sits above it) and duplicates no fee schedule.
//!
//! **The pre-registered decision is a typed input** (DEC-122): the calendar window a scored
//! thesis's horizon must close inside, the minimum count of *scoreable* closed theses, and the
//! one-sided bound's `z` as an exact decimal (1.645 for the 95% DEC-122 registers). The
//! evaluator refuses an early run — fewer scoreable closed theses than the minimum — because a
//! report before the window closes would be the choose-the-answer-after-the-fact that
//! pre-registration exists to prevent, and it refuses a thesis whose horizon closes outside the
//! registered window: the caller selects, the evaluator re-checks, and nothing is silently
//! included or dropped.
//!
//! An **empty scoreable set** is its own refusal (DEC-335): no theses at all, or every thesis
//! unscoreable, leaves nothing to aggregate, and the mean of nothing is not a report (#410
//! review minor 3, DEC-282 item 9). The arm's reach is the nameless refusal's own — an empty
//! set the count refusal does not answer, a registered minimum of zero, the only minimum an
//! empty set is not below; below a positive minimum the count refusal keeps the empty set, as
//! #410's round-1 pins freeze it. Input integrity keeps its precedence: a thesis whose horizon
//! closes outside the registered window is refused before the empty set is ever reached
//! (DEC-282 item 3).
//!
//! **The boundary rule is no-lookahead at both edges** (R-27's whole point): the entry price is
//! the first close *strictly after* the thesis's `as_of` **and at or before the horizon end** —
//! the first price inside the window the thesis claims; a close at the same instant is the
//! model's own, not one it could trade, and a close past the horizon is a price the thesis could
//! never have acted on either — and the exit price is the last close *at or before* the horizon
//! end, so nothing past the horizon is ever read, at either edge. An edge with no close on its
//! side of the rule makes the thesis **unscoreable**: excluded, named with its reason, and never
//! counted toward the mean *or* the minimum, so a data gap can never help an evaluation pass —
//! a thesis whose closes all lie outside its window, and the degenerate window whose horizon
//! closes at or before its own `as_of`, are unscoreable the same way, never a figure
//! (DEC-282 item 8).
//!
//! The scorecard is the report and recomputes from its own fields, its rows ordered by thesis
//! id, so the same journal and the same series give an identical scorecard every run.

use std::collections::BTreeMap;

use mandate_num::{NumError, Price, Ratio, Rounding, SignedQty};
use mandate_time::UtcNanos;

use crate::{AssetId, Direction, LineageId, ResearchError, ThesisId};

/// Every figure the report rounds is rounded at 12 fractional digits (DEC-127 item 4,
/// DEC-281) — the same scale the E4-2 report rounds at, pinned live in this module's tests.
pub const REPORT_SCALE: u32 = 12;

/// The pre-registered calendar window (DEC-122): a scored thesis's horizon must close inside
/// `[from, to]`, and the evaluator refuses one that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvaluationWindow {
    /// The first instant a scored horizon may close at.
    pub from: UtcNanos,
    /// The last instant a scored horizon may close at, inclusive.
    pub to: UtcNanos,
}

/// The recorded decision the evaluation runs under (DEC-122, DEC-281 item 2): set before the
/// evaluation starts, applied as registered, never chosen after the fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationDecision {
    /// The calendar window a scored thesis's horizon must close inside.
    pub window: EvaluationWindow,
    /// The minimum count of scoreable closed theses before the evaluation may report.
    pub minimum_scoreable: u32,
    /// The one-sided bound's `z` as an exact decimal (DEC-122's 95% is 1.645).
    pub z: Ratio,
}

/// One observed close: an instant and the price closed at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedClose {
    /// When the close was observed.
    pub at: UtcNanos,
    /// The close price.
    pub price: Price,
}

/// One instrument's observed closes, in observation order — the evaluator's only view of market
/// data, typed here because the market-data crates sit above this crate (ES-02). The
/// constructor refuses an empty series and any instants that do not strictly advance, so the
/// boundary rules below never read an ambiguous order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseSeries {
    instrument: AssetId,
    closes: Vec<ObservedClose>,
}

impl CloseSeries {
    /// Builds a series from observed closes, refusing an empty one and one whose instants do
    /// not strictly advance.
    ///
    /// # Errors
    /// Returns [`ResearchError::EmptyCloses`] for no closes and
    /// [`ResearchError::ClosesOutOfOrder`] for instants that repeat or go backwards.
    pub fn new(instrument: AssetId, closes: Vec<ObservedClose>) -> Result<Self, ResearchError> {
        if closes.is_empty() {
            return Err(ResearchError::EmptyCloses);
        }
        let mut previous: Option<UtcNanos> = None;
        for observed in &closes {
            if previous.is_some_and(|at| observed.at <= at) {
                return Err(ResearchError::ClosesOutOfOrder);
            }
            previous = Some(observed.at);
        }
        Ok(Self { instrument, closes })
    }
}

/// One closed thesis as the journal holds it, with the modeled cost the caller computed: the
/// fields the evaluator reads and nothing else. No admission outcome and no predecessor's score
/// exist on it, so a refused thesis scores like an admitted one and a revision starts from zero
/// (MI-18, DEC-281 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedThesis {
    /// The thesis's own id.
    pub thesis: ThesisId,
    /// The lineage the thesis belongs to.
    pub lineage: LineageId,
    /// The thesis's revision within its lineage, shown on its scored row (MI-18).
    pub revision: u32,
    /// The instrument the thesis is about.
    pub instrument: AssetId,
    /// The thesis's direction; v1 scores `Long` only.
    pub direction: Direction,
    /// The model's knowledge instant (§8.2's `as_of`): the entry edge reads the first close
    /// strictly after it, inside the horizon.
    pub as_of: UtcNanos,
    /// The horizon end (§8.2's `expires_at`): the exit edge reads the last close at or before
    /// it, and the horizon must close inside the registered window.
    pub horizon_end: UtcNanos,
    /// The modeled round-trip cost as an exact fraction of notional, computed by the caller
    /// from the same §6.4 fee model the backtest charges.
    pub round_trip_cost: Ratio,
}

/// Everything one evaluation reads: the registered decision, the window's closed theses, the
/// close series of every instrument a thesis names, the eligible basket's members, and the
/// broad index ETF.
#[derive(Debug, Clone, Copy)]
pub struct EvaluationInput<'a> {
    /// The decision the evaluation runs under.
    pub decision: &'a EvaluationDecision,
    /// The closed theses of the registered window, as the shell selected them.
    pub theses: &'a [ClosedThesis],
    /// The close series of every instrument a thesis names, keyed by id.
    pub instruments: &'a BTreeMap<AssetId, CloseSeries>,
    /// The eligible basket's members (DEC-90), each its own series.
    pub basket: &'a [CloseSeries],
    /// The broad index ETF's series.
    pub index: &'a CloseSeries,
}

/// Why a closed thesis cannot be scored (DEC-281 item 3): named on the scorecard, never a
/// silent drop, and never counted toward the mean or the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnscoreableReason {
    /// No close strictly after `as_of` and at or before the horizon end: the entry edge has no
    /// price inside the thesis's own window.
    NoEntryClose,
    /// No close at or before the horizon end: the exit edge has no price.
    NoExitClose,
    /// The thesis's instrument has no series in the input.
    NoSeries,
    /// The thesis's direction is not scoreable in v1 (only `Long` is, DEC-32).
    NotScoreableDirection,
}

/// One closed thesis the evaluation could not score, named with why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unscoreable {
    /// The thesis that could not be scored.
    pub thesis: ThesisId,
    /// Why.
    pub reason: UnscoreableReason,
}

/// One scored thesis's row: what it scored from and what it scored, every figure exact or one
/// named rounding (DEC-281 items 4 and 5). The row carries its own round-trip cost, so its net
/// return recomputes from the row alone (DEC-127 item 5's precedent: the report shows what a
/// reader needs to recompute it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoredThesis {
    /// The thesis's own id.
    pub thesis: ThesisId,
    /// The lineage the thesis belongs to.
    pub lineage: LineageId,
    /// The thesis's revision within its lineage (MI-18: every report shows it).
    pub revision: u32,
    /// The instrument the thesis is about.
    pub instrument: AssetId,
    /// The entry price the score used: the first close strictly after `as_of`, at or before the
    /// horizon end.
    pub entry: Price,
    /// The exit price the score used: the last close at or before the horizon end.
    pub exit: Price,
    /// The modeled round-trip cost the score subtracted, echoed so the net return recomputes.
    pub round_trip_cost: Ratio,
    /// The direction's window return net of the round-trip cost.
    pub net_return: Ratio,
    /// The net return less the basket baseline's return over the same window.
    pub excess_over_basket: Ratio,
    /// The net return less the index baseline's return over the same window.
    pub excess_over_index: Ratio,
}

/// The evaluation's report: every figure recomputes from the fields above it (the E4-2
/// discipline) — the mean from the excess sum and the scoreable count, the margin from the
/// echoed `z`, the variance, and the count, the bound from the mean less the margin, and pass
/// from both bounds (DEC-281 item 7, DEC-127 item 5's precedent that the report carries what a
/// reader needs to recompute it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scorecard {
    /// The scored rows, ordered by thesis id.
    pub theses: Vec<ScoredThesis>,
    /// The closed theses that could not be scored, named with why, ordered by thesis id so the
    /// report never depends on input order.
    pub unscoreable: Vec<Unscoreable>,
    /// How many theses were scored; the mean and the minimum read this count alone.
    pub scoreable_count: u32,
    /// The decision's `z`, echoed so the bound recomputes from the report alone.
    pub z: Ratio,
    /// The sum of the scored excesses over the basket baseline.
    pub excess_sum_basket: Ratio,
    /// The sum of the scored excesses over the index baseline.
    pub excess_sum_index: Ratio,
    /// The mean excess per scored thesis over the basket baseline.
    pub mean_excess_basket: Ratio,
    /// The mean excess per scored thesis over the index baseline.
    pub mean_excess_index: Ratio,
    /// The sample variance of the basket excesses; absent below two theses.
    pub sample_variance_basket: Option<Ratio>,
    /// The sample variance of the index excesses; absent below two theses.
    pub sample_variance_index: Option<Ratio>,
    /// The basket bound's margin `z × σ ÷ √n`, ceilinged so it is never understated.
    pub margin_basket: Ratio,
    /// The index bound's margin.
    pub margin_index: Ratio,
    /// The basket bound: the mean less the margin.
    pub lower_bound_basket: Ratio,
    /// The index bound.
    pub lower_bound_index: Ratio,
    /// Whether the bound is above zero against **both** baselines (DEC-122's "against each").
    pub passed: bool,
}

/// The first close strictly after `as_of` — the search the frozen entry boundary and the scoring
/// path's own windowed entry share, so the strictly-after rule exists once.
fn first_close_after(series: &CloseSeries, as_of: UtcNanos) -> Option<&ObservedClose> {
    series.closes.iter().find(|observed| observed.at > as_of)
}

/// The entry boundary (DEC-281 item 3): the first close strictly after `as_of`, the first
/// price the thesis could have acted on. A close at the same instant is not it.
///
/// # Errors
/// Returns [`ResearchError::NoCloseAfter`] when the series holds no close after the instant.
pub fn entry_close(series: &CloseSeries, as_of: UtcNanos) -> Result<Price, ResearchError> {
    first_close_after(series, as_of)
        .map(|observed| observed.price)
        .ok_or(ResearchError::NoCloseAfter)
}

/// The scoring path's entry boundary (DEC-282 item 8): the first close strictly after `as_of`
/// **and at or before `horizon_end`** — the first price inside the window the thesis claims, so
/// a close past the horizon is never read at the entry either. A first close that lies past the
/// horizon leaves the window without a price the thesis could have acted on: no entry, the same
/// [`ResearchError::NoCloseAfter`] the frozen boundary returns when no close follows `as_of` at
/// all.
///
/// # Errors
/// Returns [`ResearchError::NoCloseAfter`] when the series holds no close strictly after
/// `as_of` and at or before `horizon_end` — a degenerate window, whose `horizon_end` sits at or
/// before its own `as_of`, holds no such close whatever the series carries.
fn entry_close_within(
    series: &CloseSeries,
    as_of: UtcNanos,
    horizon_end: UtcNanos,
) -> Result<Price, ResearchError> {
    let observed = first_close_after(series, as_of).ok_or(ResearchError::NoCloseAfter)?;
    if observed.at > horizon_end {
        return Err(ResearchError::NoCloseAfter);
    }
    Ok(observed.price)
}

/// The exit boundary (DEC-281 item 3): the last close at or before the horizon end — nothing
/// past the horizon is ever read.
///
/// # Errors
/// Returns [`ResearchError::NoCloseOnOrBefore`] when the series holds no close at or before the
/// instant.
pub fn exit_close(series: &CloseSeries, horizon_end: UtcNanos) -> Result<Price, ResearchError> {
    series
        .closes
        .iter()
        .rev()
        .find(|observed| observed.at <= horizon_end)
        .map(|observed| observed.price)
        .ok_or(ResearchError::NoCloseOnOrBefore)
}

/// Buy-and-hold over the window the boundary rules pick — the exit the last close at or before
/// `horizon_end`, the entry the first close strictly after `as_of` and at or before
/// `horizon_end` (DEC-282 item 8, so a close past the horizon is never read at either edge):
/// `(exit − entry) ÷ entry`, one rounding at 12 places half-even (DEC-127 item 4's report
/// scale).
///
/// # Errors
/// Returns [`ResearchError`] for either edge's boundary failure, and
/// [`ResearchError::Num`] for a non-positive entry price, which a [`Price`] never is.
pub fn buy_and_hold(
    series: &CloseSeries,
    as_of: UtcNanos,
    horizon_end: UtcNanos,
) -> Result<Ratio, ResearchError> {
    let exit = exit_close(series, horizon_end)?;
    let entry = entry_close_within(series, as_of, horizon_end)?;
    window_return(entry, exit)
}

/// The basket baseline (DEC-281 item 5): the equal-weighted mean of the members' own window
/// returns.
///
/// # Errors
/// Returns [`ResearchError`] for a member's boundary failure, and
/// [`ResearchError::Num`] for an empty basket, whose mean is undefined.
pub fn basket_return(
    members: &[CloseSeries],
    as_of: UtcNanos,
    horizon_end: UtcNanos,
) -> Result<Ratio, ResearchError> {
    let returns = members
        .iter()
        .map(|member| buy_and_hold(member, as_of, horizon_end))
        .collect::<Result<Vec<_>, _>>()?;
    Ratio::mean(&returns).map_err(ResearchError::Num)
}

/// The evaluation itself (DEC-281 items 2 to 7): scores every thesis it can, names the ones it
/// cannot, refuses the early run and the thesis outside the registered window, and reports the
/// mean excess, the one-sided bound, and pass against both baselines.
///
/// # Errors
/// Returns [`ResearchError::ThesisOutsideWindow`] for a thesis whose horizon closes outside
/// the registered window, [`ResearchError::EmptyScoreableSet`] for an evaluation whose
/// scoreable set is empty — no theses at all, or every thesis unscoreable — where the count
/// refusal does not answer it, a registered minimum of zero (DEC-335),
/// [`ResearchError::WindowNotClosed`] when fewer than the minimum scoreable closed theses are
/// scoreable, and [`ResearchError::Num`] for the arithmetic a figure cannot express.
pub fn evaluate(input: &EvaluationInput<'_>) -> Result<Scorecard, ResearchError> {
    for thesis in input.theses {
        if thesis.horizon_end < input.decision.window.from
            || thesis.horizon_end > input.decision.window.to
        {
            return Err(ResearchError::ThesisOutsideWindow);
        }
    }
    let mut theses = Vec::new();
    let mut unscoreable = Vec::new();
    for thesis in input.theses {
        if thesis.direction != Direction::Long {
            unscoreable.push(Unscoreable {
                thesis: thesis.thesis.clone(),
                reason: UnscoreableReason::NotScoreableDirection,
            });
            continue;
        }
        let Some(series) = input.instruments.get(&thesis.instrument) else {
            unscoreable.push(Unscoreable {
                thesis: thesis.thesis.clone(),
                reason: UnscoreableReason::NoSeries,
            });
            continue;
        };
        let exit = match exit_close(series, thesis.horizon_end) {
            Ok(price) => price,
            Err(ResearchError::NoCloseOnOrBefore) => {
                unscoreable.push(Unscoreable {
                    thesis: thesis.thesis.clone(),
                    reason: UnscoreableReason::NoExitClose,
                });
                continue;
            }
            Err(other) => return Err(other),
        };
        let entry = match entry_close_within(series, thesis.as_of, thesis.horizon_end) {
            Ok(price) => price,
            Err(ResearchError::NoCloseAfter) => {
                unscoreable.push(Unscoreable {
                    thesis: thesis.thesis.clone(),
                    reason: UnscoreableReason::NoEntryClose,
                });
                continue;
            }
            Err(other) => return Err(other),
        };
        let gross = window_return(entry, exit)?;
        let net = gross.checked_sub(thesis.round_trip_cost)?;
        let basket = basket_return(input.basket, thesis.as_of, thesis.horizon_end)?;
        let index = buy_and_hold(input.index, thesis.as_of, thesis.horizon_end)?;
        theses.push(ScoredThesis {
            thesis: thesis.thesis.clone(),
            lineage: thesis.lineage.clone(),
            revision: thesis.revision,
            instrument: thesis.instrument.clone(),
            entry,
            exit,
            round_trip_cost: thesis.round_trip_cost,
            net_return: net,
            excess_over_basket: net.checked_sub(basket)?,
            excess_over_index: net.checked_sub(index)?,
        });
    }
    theses.sort_by(|left, right| left.thesis.as_str().cmp(right.thesis.as_str()));
    unscoreable.sort_by(|left, right| left.thesis.as_str().cmp(right.thesis.as_str()));
    let count = u32::try_from(theses.len()).map_err(|_| ResearchError::Num(NumError::Overflow))?;
    if count < input.decision.minimum_scoreable {
        return Err(ResearchError::WindowNotClosed);
    }
    if count == 0 {
        return Err(ResearchError::EmptyScoreableSet);
    }
    let excesses_basket: Vec<Ratio> = theses.iter().map(|row| row.excess_over_basket).collect();
    let excesses_index: Vec<Ratio> = theses.iter().map(|row| row.excess_over_index).collect();
    let (sum_basket, mean_basket, variance_basket, margin_basket) =
        aggregate(&excesses_basket, input.decision.z, count)?;
    let (sum_index, mean_index, variance_index, margin_index) =
        aggregate(&excesses_index, input.decision.z, count)?;
    let lower_bound_basket = mean_basket.checked_sub(margin_basket)?;
    let lower_bound_index = mean_index.checked_sub(margin_index)?;
    Ok(Scorecard {
        theses,
        unscoreable,
        scoreable_count: count,
        z: input.decision.z,
        excess_sum_basket: sum_basket,
        excess_sum_index: sum_index,
        mean_excess_basket: mean_basket,
        mean_excess_index: mean_index,
        sample_variance_basket: variance_basket,
        sample_variance_index: variance_index,
        margin_basket,
        margin_index,
        lower_bound_basket,
        lower_bound_index,
        passed: lower_bound_basket > Ratio::ZERO && lower_bound_index > Ratio::ZERO,
    })
}

/// `(exit − entry) ÷ entry`, one rounding at 12 places half-even (DEC-281 item 4): the window
/// return every edge of a buy-and-hold shares, computed as a unit share's value at each price
/// through the same `Usd::ratio_to` the E4-2 report rides, so no new `Price` arithmetic exists
/// here to drift from it (DEC-282 item 1).
fn window_return(entry: Price, exit: Price) -> Result<Ratio, ResearchError> {
    let one = SignedQty::parse("1")?;
    let entry_usd = one.value_at(entry)?;
    let exit_usd = one.value_at(exit)?;
    Ok(exit_usd
        .checked_sub(entry_usd)?
        .ratio_to(entry_usd, REPORT_SCALE, Rounding::HalfEven)?)
}

/// One baseline's excess figures: the sum and mean over the scoreable rows, the sample variance
/// (absent below two, when there is no dispersion to state), and the margin
/// `root_ceiling(squared_quotient(z × σ, n))` with σ the variance's own ceiling root — never
/// understated at either root (DEC-281 item 6).
fn aggregate(
    excesses: &[Ratio],
    z: Ratio,
    count: u32,
) -> Result<(Ratio, Ratio, Option<Ratio>, Ratio), ResearchError> {
    let sum = Ratio::sum(excesses)?;
    let mean = Ratio::mean(excesses)?;
    if count < 2 {
        return Ok((sum, mean, None, Ratio::ZERO));
    }
    let squares = Ratio::sum_of_squares(excesses)?;
    let variance = Ratio::sample_variance(sum, squares, count)?;
    let sigma = variance.root_ceiling()?;
    let margin = Ratio::squared_quotient(z.checked_mul(sigma)?, Ratio::parse(&count.to_string())?)?
        .root_ceiling()?;
    Ok((sum, mean, Some(variance), margin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The stable codes (ES-09) of the errors this module mints, pinned live because the
    /// pending E17-8 tests do not run under the mutation gate (DEC-253 item 2), so a code arm
    /// no live test reads would be a mutant nothing catches.
    #[test]
    fn the_score_codes_are_pinned() {
        assert_eq!(
            ResearchError::ClosesOutOfOrder.code(),
            "closes_out_of_order"
        );
        assert_eq!(ResearchError::EmptyCloses.code(), "empty_closes");
        assert_eq!(ResearchError::NoCloseAfter.code(), "no_close_after");
        assert_eq!(
            ResearchError::NoCloseOnOrBefore.code(),
            "no_close_on_or_before"
        );
        assert_eq!(ResearchError::WindowNotClosed.code(), "window_not_closed");
        assert_eq!(
            ResearchError::ThesisOutsideWindow.code(),
            "thesis_outside_window"
        );
        assert_eq!(
            ResearchError::EmptyScoreableSet.code(),
            "empty_scoreable_set"
        );
    }

    /// The report's rounding scale, pinned live (DEC-127 item 4's, the same scale every figure
    /// of the E4-2 report rounds at): a scale moved by one place changes every rounded figure.
    #[test]
    fn the_report_scale_is_pinned() {
        assert_eq!(REPORT_SCALE, 12);
    }

    /// `t(secs)`: an instant at a whole second, for the module's own cases.
    fn t(secs: i64) -> Result<UtcNanos, ResearchError> {
        UtcNanos::from_parts(secs, 0).map_err(ResearchError::Time)
    }

    /// One Long thesis over `[t(100), t(1000)]` with no round-trip cost.
    fn long_thesis(id: &str, instrument: &str) -> Result<ClosedThesis, ResearchError> {
        Ok(ClosedThesis {
            thesis: ThesisId::new(id)?,
            lineage: LineageId::new(id)?,
            revision: 0,
            instrument: AssetId::new(instrument)?,
            direction: Direction::Long,
            as_of: t(100)?,
            horizon_end: t(1_000)?,
            round_trip_cost: Ratio::ZERO,
        })
    }

    /// A series of two closes, at `t(101)` and `t(999)`.
    fn two_closes(instrument: &str, first: &str, last: &str) -> Result<CloseSeries, ResearchError> {
        CloseSeries::new(
            AssetId::new(instrument)?,
            vec![
                ObservedClose {
                    at: t(101)?,
                    price: Price::parse(first)?,
                },
                ObservedClose {
                    at: t(999)?,
                    price: Price::parse(last)?,
                },
            ],
        )
    }

    /// DEC-282 item 4: pass is strictly above zero — a bound of exactly zero is not a pass, on
    /// either side. Three cases with a registered `z` of zero (so the margin is zero and each
    /// bound is its own mean): both means zero, the basket mean alone zero against a positive
    /// index mean, and the index mean alone zero against a positive basket mean — the last two
    /// are the ones a single relaxed comparison would flip, since the other bound's strict
    /// comparison would still hold `passed` back.
    #[test]
    fn a_bound_of_exactly_zero_does_not_pass() -> Result<(), ResearchError> {
        let rising = long_thesis("th-1", "asset-a")?;
        let falling = long_thesis("th-2", "asset-b")?;
        let instruments = BTreeMap::from([
            (
                AssetId::new("asset-a")?,
                two_closes("asset-a", "100", "110")?,
            ),
            (
                AssetId::new("asset-b")?,
                two_closes("asset-b", "100", "90")?,
            ),
        ]);
        let basket_of = |exit: &str| -> Result<Vec<CloseSeries>, ResearchError> {
            Ok(vec![two_closes("basket-1", "100", exit)?])
        };
        let card_of = |basket_exit: &str, index_exit: &str| -> Result<Scorecard, ResearchError> {
            let registered = EvaluationDecision {
                window: EvaluationWindow {
                    from: t(0)?,
                    to: t(10_000)?,
                },
                minimum_scoreable: 2,
                z: Ratio::parse("0")?,
            };
            evaluate(&EvaluationInput {
                decision: &registered,
                theses: &[rising.clone(), falling.clone()],
                instruments: &instruments,
                basket: &basket_of(basket_exit)?,
                index: &two_closes("index", "400", index_exit)?,
            })
        };
        let both_zero = card_of("100", "400")?;
        assert_eq!(both_zero.mean_excess_basket, Ratio::parse("0")?);
        assert_eq!(
            both_zero.sample_variance_basket,
            Some(Ratio::parse("0.02")?),
            "(2 × 0.02 − 0) ÷ 2 over the excesses [0.1, −0.1]"
        );
        assert_eq!(both_zero.margin_basket, Ratio::ZERO, "z = 0: no margin");
        assert_eq!(both_zero.lower_bound_basket, Ratio::parse("0")?);
        assert_eq!(both_zero.lower_bound_index, Ratio::parse("0")?);
        assert!(
            !both_zero.passed,
            "DEC-122's threshold is the bound above zero, not at it"
        );
        let basket_zero = card_of("100", "200")?;
        assert_eq!(basket_zero.lower_bound_basket, Ratio::parse("0")?);
        assert_eq!(
            basket_zero.lower_bound_index,
            Ratio::parse("0.5")?,
            "a falling index (−0.5) puts the index mean at 0.5 over the excesses [0.6, 0.4]"
        );
        assert!(
            !basket_zero.passed,
            "the basket bound sits at exactly zero while the index bound is above it: still a fail"
        );
        let index_zero = card_of("50", "400")?;
        assert_eq!(
            index_zero.lower_bound_basket,
            Ratio::parse("0.5")?,
            "a falling basket member (−0.5) puts the basket mean at 0.5 over the excesses [0.6, 0.4]"
        );
        assert_eq!(index_zero.lower_bound_index, Ratio::parse("0")?);
        assert!(
            !index_zero.passed,
            "the index bound sits at exactly zero while the basket bound is above it: still a fail"
        );
        Ok(())
    }

    /// DEC-282 item 3: an unscoreable thesis's reason is the first failing one, in the order the
    /// input presents it — the direction (part of the thesis value itself), then the series (the
    /// instruments map), then the edges (the series) — so a non-Long thesis whose instrument also
    /// has no series is `NotScoreableDirection`, not `NoSeries`.
    #[test]
    fn the_first_failing_unscoreable_reason_is_reported() -> Result<(), ResearchError> {
        let scores = long_thesis("th-1", "asset-a")?;
        let mut other = long_thesis("th-2", "asset-b")?;
        other.direction = Direction::Other;
        let instruments = BTreeMap::from([(
            AssetId::new("asset-a")?,
            two_closes("asset-a", "100", "120")?,
        )]);
        let basket = vec![two_closes("basket-1", "100", "100")?];
        let index = two_closes("index", "400", "400")?;
        let registered = EvaluationDecision {
            window: EvaluationWindow {
                from: t(0)?,
                to: t(10_000)?,
            },
            minimum_scoreable: 1,
            z: Ratio::parse("1.645")?,
        };
        let card = evaluate(&EvaluationInput {
            decision: &registered,
            theses: &[scores, other],
            instruments: &instruments,
            basket: &basket,
            index: &index,
        })?;
        assert_eq!(card.scoreable_count, 1);
        assert_eq!(
            card.unscoreable.first().map(|each| each.reason),
            Some(UnscoreableReason::NotScoreableDirection),
            "the direction is checked before the series is looked up"
        );
        Ok(())
    }

    /// DEC-282 item 8 (#410 round 1's blocker): a thesis whose closes all lie outside its own
    /// window — none strictly after `as_of` and at or before `horizon_end` — has no entry
    /// price: unscoreable with `NoEntryClose`, never counted toward the minimum. The unbounded
    /// entry read the close at t(9000), 8000 seconds past the horizon, and scored a backwards
    /// window against the close at t(50), before `as_of`; both price orders are pinned, and the
    /// close at `as_of` itself stays the model's own, never an entry.
    #[test]
    fn a_thesis_whose_closes_all_lie_outside_its_window_is_unscoreable() -> Result<(), ResearchError>
    {
        let scored = long_thesis("th-1", "asset-a")?;
        let gapped = long_thesis("th-2", "asset-b")?;
        let basket = vec![two_closes("basket-1", "100", "100")?];
        let index = two_closes("index", "400", "400")?;
        let registered = EvaluationDecision {
            window: EvaluationWindow {
                from: t(0)?,
                to: t(10_000)?,
            },
            minimum_scoreable: 1,
            z: Ratio::parse("1.645")?,
        };
        let gapped_series = |late: &str, early: &str| -> Result<CloseSeries, ResearchError> {
            CloseSeries::new(
                AssetId::new("asset-b")?,
                vec![
                    ObservedClose {
                        at: t(50)?,
                        price: Price::parse(early)?,
                    },
                    ObservedClose {
                        at: t(100)?,
                        price: Price::parse("250")?,
                    },
                    ObservedClose {
                        at: t(9000)?,
                        price: Price::parse(late)?,
                    },
                ],
            )
        };
        let card_of = |late: &str, early: &str| -> Result<Scorecard, ResearchError> {
            let instruments = BTreeMap::from([
                (
                    AssetId::new("asset-a")?,
                    two_closes("asset-a", "100", "120")?,
                ),
                (AssetId::new("asset-b")?, gapped_series(late, early)?),
            ]);
            evaluate(&EvaluationInput {
                decision: &registered,
                theses: &[scored.clone(), gapped.clone()],
                instruments: &instruments,
                basket: &basket,
                index: &index,
            })
        };
        for (late, early) in [("100", "500"), ("500", "100")] {
            let card = card_of(late, early)?;
            assert_eq!(
                card.scoreable_count, 1,
                "the gap thesis never scores, whichever way its outside-window prices lean"
            );
            assert_eq!(
                card.unscoreable,
                vec![Unscoreable {
                    thesis: ThesisId::new("th-2")?,
                    reason: UnscoreableReason::NoEntryClose,
                }],
                "the thesis's only closes sit before `as_of`, at `as_of` itself, and past the horizon: no price inside the window it claims"
            );
        }
        let instruments =
            BTreeMap::from([(AssetId::new("asset-b")?, gapped_series("100", "500")?)]);
        assert!(
            matches!(
                evaluate(&EvaluationInput {
                    decision: &registered,
                    theses: &[gapped],
                    instruments: &instruments,
                    basket: &basket,
                    index: &index,
                }),
                Err(ResearchError::WindowNotClosed)
            ),
            "one unscoreable thesis against a minimum of one: the gap does not close the window"
        );
        Ok(())
    }

    /// DEC-282 item 8's degenerate window (#410 round 1's second-order finding): a horizon that
    /// closes at or before the thesis's own `as_of` leaves the entry interval empty, so the
    /// thesis is unscoreable, never a figure — the review's fixture (closes at t(50) and
    /// t(150), `as_of` t(140), `horizon_end` t(60)) scored −0.5 off the inverted window. Pinned
    /// at both edges of the degeneracy: the horizon strictly before `as_of`, and exactly at it.
    #[test]
    fn a_degenerate_window_is_unscoreable_never_a_figure() -> Result<(), ResearchError> {
        let scored = long_thesis("th-1", "asset-a")?;
        let mut inverted = long_thesis("th-2", "asset-b")?;
        inverted.as_of = t(140)?;
        inverted.horizon_end = t(60)?;
        let mut empty_interval = long_thesis("th-3", "asset-c")?;
        empty_interval.as_of = t(100)?;
        empty_interval.horizon_end = t(100)?;
        let degenerate_series = |instrument: &str| -> Result<CloseSeries, ResearchError> {
            CloseSeries::new(
                AssetId::new(instrument)?,
                vec![
                    ObservedClose {
                        at: t(50)?,
                        price: Price::parse("100")?,
                    },
                    ObservedClose {
                        at: t(150)?,
                        price: Price::parse("200")?,
                    },
                ],
            )
        };
        let instruments = BTreeMap::from([
            (
                AssetId::new("asset-a")?,
                two_closes("asset-a", "100", "120")?,
            ),
            (AssetId::new("asset-b")?, degenerate_series("asset-b")?),
            (AssetId::new("asset-c")?, degenerate_series("asset-c")?),
        ]);
        let basket = vec![two_closes("basket-1", "100", "100")?];
        let index = two_closes("index", "400", "400")?;
        let registered = EvaluationDecision {
            window: EvaluationWindow {
                from: t(0)?,
                to: t(10_000)?,
            },
            minimum_scoreable: 1,
            z: Ratio::parse("1.645")?,
        };
        let card = evaluate(&EvaluationInput {
            decision: &registered,
            theses: &[scored, inverted.clone(), empty_interval.clone()],
            instruments: &instruments,
            basket: &basket,
            index: &index,
        })?;
        assert_eq!(card.scoreable_count, 1, "only the healthy thesis scores");
        assert_eq!(
            card.unscoreable,
            vec![
                Unscoreable {
                    thesis: ThesisId::new("th-2")?,
                    reason: UnscoreableReason::NoEntryClose,
                },
                Unscoreable {
                    thesis: ThesisId::new("th-3")?,
                    reason: UnscoreableReason::NoEntryClose,
                },
            ],
            "a horizon at or before `as_of` holds no close strictly after `as_of` and at or before itself: the entry edge has no price"
        );
        let strict = EvaluationDecision {
            window: EvaluationWindow {
                from: t(0)?,
                to: t(10_000)?,
            },
            minimum_scoreable: 2,
            z: Ratio::parse("1.645")?,
        };
        assert!(
            matches!(
                evaluate(&EvaluationInput {
                    decision: &strict,
                    theses: &[inverted, empty_interval],
                    instruments: &instruments,
                    basket: &basket,
                    index: &index,
                }),
                Err(ResearchError::WindowNotClosed)
            ),
            "two degenerate windows against a minimum of two: no figure, and the window does not close"
        );
        Ok(())
    }

    /// DEC-282 item 8's upper edge: a close exactly at `horizon_end` is inside the window — it
    /// is the entry when it is the first close after `as_of`, and the exit besides, so the
    /// window return is a figure (zero), not a refusal; the closes around it, before `as_of`
    /// and past the horizon, are never read.
    #[test]
    fn a_close_exactly_at_the_horizon_is_the_entry_and_the_exit() -> Result<(), ResearchError> {
        let at_horizon = long_thesis("th-1", "asset-a")?;
        let instruments = BTreeMap::from([(
            AssetId::new("asset-a")?,
            CloseSeries::new(
                AssetId::new("asset-a")?,
                vec![
                    ObservedClose {
                        at: t(50)?,
                        price: Price::parse("300")?,
                    },
                    ObservedClose {
                        at: t(1000)?,
                        price: Price::parse("120")?,
                    },
                    ObservedClose {
                        at: t(2000)?,
                        price: Price::parse("999")?,
                    },
                ],
            )?,
        )]);
        let basket = vec![two_closes("basket-1", "100", "100")?];
        let index = two_closes("index", "400", "400")?;
        let registered = EvaluationDecision {
            window: EvaluationWindow {
                from: t(0)?,
                to: t(10_000)?,
            },
            minimum_scoreable: 1,
            z: Ratio::parse("1.645")?,
        };
        let card = evaluate(&EvaluationInput {
            decision: &registered,
            theses: &[at_horizon],
            instruments: &instruments,
            basket: &basket,
            index: &index,
        })?;
        assert_eq!(card.scoreable_count, 1);
        assert_eq!(
            card.theses
                .first()
                .map(|row| (row.entry, row.exit, row.net_return)),
            Some((
                Price::parse("120")?,
                Price::parse("120")?,
                Ratio::parse("0")?
            )),
            "the close at the horizon itself is the first close inside the window, and the last close at or before the horizon: entry and exit are one close, so the window return is zero, a figure"
        );
        Ok(())
    }

    /// DEC-282 item 3's precedence with both refusals live (#410 round 1, minor 2): a thesis
    /// whose horizon closes outside the registered window is refused before the scoreable count
    /// is consulted, so the answer names the input-integrity failure even when the scoreable
    /// theses also fall below the registered minimum — the same input without the outside
    /// thesis refuses with `WindowNotClosed`, so both refusals are live and the order is what
    /// this pin reads.
    #[test]
    fn a_thesis_outside_the_window_is_refused_before_the_minimum() -> Result<(), ResearchError> {
        let first = long_thesis("th-1", "asset-a")?;
        let second = long_thesis("th-2", "asset-b")?;
        let mut outside = long_thesis("th-3", "asset-c")?;
        outside.horizon_end = t(20_000)?;
        let instruments = BTreeMap::from([
            (
                AssetId::new("asset-a")?,
                two_closes("asset-a", "100", "120")?,
            ),
            (
                AssetId::new("asset-b")?,
                two_closes("asset-b", "100", "110")?,
            ),
            (
                AssetId::new("asset-c")?,
                two_closes("asset-c", "100", "100")?,
            ),
        ]);
        let basket = vec![two_closes("basket-1", "100", "100")?];
        let index = two_closes("index", "400", "400")?;
        let strict = EvaluationDecision {
            window: EvaluationWindow {
                from: t(0)?,
                to: t(10_000)?,
            },
            minimum_scoreable: 5,
            z: Ratio::parse("1.645")?,
        };
        assert!(
            matches!(
                evaluate(&EvaluationInput {
                    decision: &strict,
                    theses: &[first.clone(), second.clone(), outside],
                    instruments: &instruments,
                    basket: &basket,
                    index: &index,
                }),
                Err(ResearchError::ThesisOutsideWindow)
            ),
            "two scoreable theses against a minimum of five, and one horizon past `to`: the outside thesis is refused first"
        );
        assert!(
            matches!(
                evaluate(&EvaluationInput {
                    decision: &strict,
                    theses: &[first, second],
                    instruments: &instruments,
                    basket: &basket,
                    index: &index,
                }),
                Err(ResearchError::WindowNotClosed)
            ),
            "the same two theses without the outside one: the count refusal is live too"
        );
        Ok(())
    }

    /// The generated scenario (DEC-282 item 5): a first thesis that always scores, up to five
    /// more that may be non-Long or lack their series, a basket of one to three members, an
    /// index, and a `z` from the registrable range. Every thesis closes inside the window, so the
    /// evaluation always reports.
    #[derive(Debug, Clone)]
    struct Generated {
        first: (u32, u32),
        extra: Vec<(bool, bool, (u32, u32))>,
        basket: Vec<(u32, u32)>,
        index: (u32, u32),
        z: &'static str,
    }

    /// A whole-number price pair whose window return stays within ±100% — the exit within
    /// `[1, 2 × entry]` — so the aggregate's Decimal-backed statistics (whose 96-bit mantissa
    /// bounds the sums of squares) never leave their bound: an evaluation wild enough to leave
    /// it refuses with `Num(Overflow)` instead, the fail-loud reading DEC-282 item 6 records.
    fn bounded_pair() -> impl Strategy<Value = (u32, u32)> {
        (1u32..500).prop_flat_map(|entry| (Just(entry), 1..=(entry.saturating_mul(2))))
    }

    /// Two closes from a generated pair: canonical, positive, and exact at 12 places.
    fn price_pair(instrument: &str, pair: (u32, u32)) -> Result<CloseSeries, ResearchError> {
        two_closes(instrument, &pair.0.to_string(), &pair.1.to_string())
    }

    /// Everything one built scenario holds: the decision, the theses, the instruments, the
    /// basket, and the index.
    type Built = (
        EvaluationDecision,
        Vec<ClosedThesis>,
        BTreeMap<AssetId, CloseSeries>,
        Vec<CloseSeries>,
        CloseSeries,
    );

    /// The scenario's values as the evaluator takes them.
    fn built(generated: &Generated) -> Result<Built, ResearchError> {
        let decision = EvaluationDecision {
            window: EvaluationWindow {
                from: t(0)?,
                to: t(10_000)?,
            },
            minimum_scoreable: 1,
            z: Ratio::parse(generated.z)?,
        };
        let mut theses = vec![long_thesis("th-1", "asset-1")?];
        let mut instruments = BTreeMap::from([(
            AssetId::new("asset-1")?,
            price_pair("asset-1", generated.first)?,
        )]);
        for (index, (is_other, has_series, pair)) in generated.extra.iter().enumerate() {
            let number = index.saturating_add(2);
            let id = format!("th-{number}");
            let asset = format!("asset-{number}");
            let mut thesis = long_thesis(&id, &asset)?;
            if *is_other {
                thesis.direction = Direction::Other;
            }
            theses.push(thesis);
            if *has_series {
                instruments.insert(AssetId::new(&asset)?, price_pair(&asset, *pair)?);
            }
        }
        let mut basket = Vec::new();
        for (index, pair) in generated.basket.iter().enumerate() {
            let number = index.saturating_add(1);
            basket.push(price_pair(&format!("basket-{number}"), *pair)?);
        }
        let index_series = price_pair("index", generated.index)?;
        Ok((decision, theses, instruments, basket, index_series))
    }

    proptest! {
        /// DEC-282 item 5: over generated scenarios the report's identities hold — the counts
        /// agree, both lists are ordered by thesis id, every aggregate figure recomputes from
        /// the report's own fields (the sums and means from the rows, the margin from the echoed
        /// `z`, the variance, and the count, the bounds from the mean less the margin, and pass
        /// from both bounds strictly above zero), each row's net return recomputes from its own
        /// entry, exit, and echoed cost, the same inputs give an identical scorecard, and neither
        /// the theses' nor the basket's input order changes it.
        #[test]
        fn the_report_s_identities_hold_over_generated_scenarios(
            generated in (
                bounded_pair(),
                proptest::collection::vec((any::<bool>(), any::<bool>(), bounded_pair()), 0..=5),
                proptest::collection::vec(bounded_pair(), 1..=3),
                bounded_pair(),
                proptest::sample::select(["0", "1", "1.645", "2"].to_vec()),
            ).prop_map(|(first, extra, basket, index, z)| Generated {
                first,
                extra,
                basket,
                index,
                z,
            }),
        ) {
            let (decision, theses, instruments, basket, index) = built(&generated)?;
            let straight = EvaluationInput {
                decision: &decision,
                theses: &theses,
                instruments: &instruments,
                basket: &basket,
                index: &index,
            };
            let card = evaluate(&straight)?;
            prop_assert_eq!(&card, &evaluate(&straight)?);
            prop_assert_eq!(
                usize::try_from(card.scoreable_count).unwrap_or(0),
                card.theses.len()
            );
            prop_assert_eq!(
                card.theses.len().saturating_add(card.unscoreable.len()),
                theses.len()
            );
            let row_ids: Vec<&str> = card.theses.iter().map(|row| row.thesis.as_str()).collect();
            prop_assert!(row_ids.is_sorted(), "the scored rows are ordered by thesis id");
            let unscored_ids: Vec<&str> = card
                .unscoreable
                .iter()
                .map(|each| each.thesis.as_str())
                .collect();
            prop_assert!(
                unscored_ids.is_sorted(),
                "the unscoreable rows are ordered by thesis id"
            );
            let basket_excesses: Vec<Ratio> = card
                .theses
                .iter()
                .map(|row| row.excess_over_basket)
                .collect();
            let index_excesses: Vec<Ratio> = card
                .theses
                .iter()
                .map(|row| row.excess_over_index)
                .collect();
            prop_assert_eq!(card.excess_sum_basket, Ratio::sum(&basket_excesses)?);
            prop_assert_eq!(card.excess_sum_index, Ratio::sum(&index_excesses)?);
            prop_assert_eq!(card.mean_excess_basket, Ratio::mean(&basket_excesses)?);
            prop_assert_eq!(card.mean_excess_index, Ratio::mean(&index_excesses)?);
            prop_assert_eq!(card.z, decision.z);
            let count = card.scoreable_count;
            match card.sample_variance_basket {
                Some(_) => prop_assert!(count >= 2),
                None => prop_assert!(count < 2),
            }
            let expected_margin = |variance: Option<Ratio>| -> Result<Ratio, ResearchError> {
                match variance {
                    Some(variance) => {
                        let sigma = variance.root_ceiling()?;
                        let product = card.z.checked_mul(sigma)?;
                        let denominator = Ratio::parse(&count.to_string())?;
                        Ok(Ratio::squared_quotient(product, denominator)?.root_ceiling()?)
                    }
                    None => Ok(Ratio::ZERO),
                }
            };
            prop_assert_eq!(
                card.margin_basket,
                expected_margin(card.sample_variance_basket)?
            );
            prop_assert_eq!(
                card.margin_index,
                expected_margin(card.sample_variance_index)?
            );
            prop_assert_eq!(
                card.lower_bound_basket,
                card.mean_excess_basket.checked_sub(card.margin_basket)?
            );
            prop_assert_eq!(
                card.lower_bound_index,
                card.mean_excess_index.checked_sub(card.margin_index)?
            );
            prop_assert_eq!(
                card.passed,
                card.lower_bound_basket > Ratio::ZERO && card.lower_bound_index > Ratio::ZERO
            );
            for row in &card.theses {
                let one = SignedQty::parse("1")?;
                let entry_usd = one.value_at(row.entry)?;
                let exit_usd = one.value_at(row.exit)?;
                let gross = exit_usd
                    .checked_sub(entry_usd)?
                    .ratio_to(entry_usd, REPORT_SCALE, Rounding::HalfEven)?;
                prop_assert_eq!(row.net_return, gross.checked_sub(row.round_trip_cost)?);
            }
            let mut reversed_theses = theses.clone();
            reversed_theses.reverse();
            let mut reversed_basket = basket.clone();
            reversed_basket.reverse();
            let reversed = EvaluationInput {
                decision: &decision,
                theses: &reversed_theses,
                instruments: &instruments,
                basket: &reversed_basket,
                index: &index,
            };
            prop_assert_eq!(&card, &evaluate(&reversed)?);
        }
    }
}
