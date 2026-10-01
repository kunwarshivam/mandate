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
//! **The boundary rule is no-lookahead at both edges** (R-27's whole point): the entry price is
//! the first close *strictly after* the thesis's `as_of` — the first price the thesis could have
//! acted on; a close at the same instant is the model's own, not one it could trade — and the
//! exit price is the last close *at or before* the horizon end, so nothing past the horizon is
//! ever read. An edge with no close on its side of the rule makes the thesis **unscoreable**:
//! excluded, named with its reason, and never counted toward the mean *or* the minimum, so a
//! data gap can never help an evaluation pass.
//!
//! The scorecard is the report and recomputes from its own fields, its rows ordered by thesis
//! id, so the same journal and the same series give an identical scorecard every run.

use std::collections::BTreeMap;

use mandate_num::{Price, Ratio};
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
        let _ = instrument;
        let _ = closes;
        Err(ResearchError::Unimplemented(
            "score::CloseSeries::new",
            "E17-8",
        ))
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
    /// strictly after it.
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
    /// No close strictly after `as_of`: the entry edge has no price.
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
/// named rounding (DEC-281 items 4 and 5).
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
    /// The entry price the score used: the first close strictly after `as_of`.
    pub entry: Price,
    /// The exit price the score used: the last close at or before the horizon end.
    pub exit: Price,
    /// The direction's window return net of the round-trip cost.
    pub net_return: Ratio,
    /// The net return less the basket baseline's return over the same window.
    pub excess_over_basket: Ratio,
    /// The net return less the index baseline's return over the same window.
    pub excess_over_index: Ratio,
}

/// The evaluation's report: every figure recomputes from the fields above it (the E4-2
/// discipline) — the mean from the excess sum and the scoreable count, the bound from the mean,
/// the variance, `z`, and the count, and pass from both bounds (DEC-281 items 6 and 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scorecard {
    /// The scored rows, ordered by thesis id.
    pub theses: Vec<ScoredThesis>,
    /// The closed theses that could not be scored, named with why, ordered by thesis id so the
    /// report never depends on input order.
    pub unscoreable: Vec<Unscoreable>,
    /// How many theses were scored; the mean and the minimum read this count alone.
    pub scoreable_count: u32,
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

/// The entry boundary (DEC-281 item 3): the first close strictly after `as_of`, the first
/// price the thesis could have acted on. A close at the same instant is not it.
///
/// # Errors
/// Returns [`ResearchError::NoCloseAfter`] when the series holds no close after the instant.
pub fn entry_close(series: &CloseSeries, as_of: UtcNanos) -> Result<Price, ResearchError> {
    let _ = (series, as_of);
    Err(ResearchError::Unimplemented("score::entry_close", "E17-8"))
}

/// The exit boundary (DEC-281 item 3): the last close at or before the horizon end — nothing
/// past the horizon is ever read.
///
/// # Errors
/// Returns [`ResearchError::NoCloseOnOrBefore`] when the series holds no close at or before the
/// instant.
pub fn exit_close(series: &CloseSeries, horizon_end: UtcNanos) -> Result<Price, ResearchError> {
    let _ = (series, horizon_end);
    Err(ResearchError::Unimplemented("score::exit_close", "E17-8"))
}

/// Buy-and-hold over the window the boundary rules pick: `(exit − entry) ÷ entry`, one rounding
/// at 12 places half-even (DEC-127 item 4's report scale).
///
/// # Errors
/// Returns [`ResearchError`] for either edge's boundary failure, and
/// [`ResearchError::Num`] for a non-positive entry price, which a [`Price`] never is.
pub fn buy_and_hold(
    series: &CloseSeries,
    as_of: UtcNanos,
    horizon_end: UtcNanos,
) -> Result<Ratio, ResearchError> {
    let _ = (series, as_of, horizon_end);
    Err(ResearchError::Unimplemented("score::buy_and_hold", "E17-8"))
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
    let _ = (members, as_of, horizon_end);
    Err(ResearchError::Unimplemented(
        "score::basket_return",
        "E17-8",
    ))
}

/// The evaluation itself (DEC-281 items 2 to 7): scores every thesis it can, names the ones it
/// cannot, refuses the early run and the thesis outside the registered window, and reports the
/// mean excess, the one-sided bound, and pass against both baselines.
///
/// # Errors
/// Returns [`ResearchError::WindowNotClosed`] when fewer than the minimum scoreable closed
/// theses are scoreable, [`ResearchError::ThesisOutsideWindow`] for a thesis whose horizon
/// closes outside the registered window, and [`ResearchError::Num`] for the arithmetic a
/// figure cannot express.
pub fn evaluate(input: &EvaluationInput<'_>) -> Result<Scorecard, ResearchError> {
    let _ = input;
    Err(ResearchError::Unimplemented("score::evaluate", "E17-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    /// The report's rounding scale, pinned live (DEC-127 item 4's, the same scale every figure
    /// of the E4-2 report rounds at): a scale moved by one place changes every rounded figure.
    #[test]
    fn the_report_scale_is_pinned() {
        assert_eq!(REPORT_SCALE, 12);
    }
}
