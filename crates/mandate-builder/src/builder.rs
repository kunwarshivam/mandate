//! The `conviction_linear` order builder
//! ([mandate spec §8.3](../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60)),
//! with the §8.1 and §8.2 signal-model contract it reads its inputs under.
//!
//! Five steps, in the order §8.3 writes them: combine, decide, size, the accumulate clips, and the
//! minimum order. Three roundings in the whole chain, all in step 1, all `round₁₂`; every other
//! value is exact (§5.2, DEC-89).
//!
//! **The mandate arrives as a narrow view.** [`BuilderMandate`] is the §8 fields only, typed, not
//! the whole [`Mandate`](mandate_spec::Mandate), whose decimals are schema text at scales no exact
//! type holds (DEC-128 item 3). `mandate-spec` supplies the conversion from a
//! [`ValidatedMandate`](mandate_spec::ValidatedMandate) without changing a signature here
//! (DEC-130 item 5); until it does, a caller builds the view and a value too wide for its type is
//! refused at that boundary rather than approximated inside the chain (item 8).

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::{AssetClass, AssetId, MarketSession, Purpose};
use mandate_num::{
    Conviction, CostBasis, FeeRate, MarkPrice, Price, Qty, SizeFraction, Unit, Usd, UsdExact,
};
use mandate_spec::document::{ModelId, SizingMethod};
use mandate_time::UtcNanos;

use crate::BuilderError;
use crate::autonomy::ActionContext;

/// A signal model's semantic version, exactly the schema's `major.minor.patch` of at most six
/// digits a part.
///
/// A grammar check with no rule logic, live in the tests PR because every test that pins a model
/// needs to name its version and a registry keyed by a type nothing can construct cannot be built
/// at all (DEC-128 item 22, the ruling that admitted `ModelId::parse`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelVersion(String);

impl ModelVersion {
    /// `^(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})$`, the schema's
    /// `$defs/signal_model/properties/version`. Leading zeros are rejected rather than folded, so
    /// two spellings of one version can never both pin a model.
    pub fn parse(text: &str) -> Result<Self, BuilderError> {
        let mut parts = text.split('.');
        for _ in 0..3 {
            let part = parts.next().ok_or(BuilderError::MalformedModelVersion)?;
            let leading_zero = part.len() > 1 && part.starts_with('0');
            if part.is_empty()
                || part.len() > 6
                || leading_zero
                || !part.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(BuilderError::MalformedModelVersion);
            }
        }
        if parts.next().is_some() {
            return Err(BuilderError::MalformedModelVersion);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A pinned signal model: the triple §8.1 pins, the owner's fixed weight, and the age its outputs
/// stay fresh for (V-007, DEC-52, DEC-67).
///
/// The weight is an envelope field the owner confirms; there is no calibration in v1 (DEC-47).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalModel {
    pub id: ModelId,
    pub version: ModelVersion,
    pub content_hash: Digest,
    pub weight: SizeFraction,
    pub max_output_age_s: u32,
}

/// A model output's direction. `long` only in v1 (DEC-32, no short sales), so a direction v1 does
/// not support is unrepresentable rather than rejected — the trust ladder's first rung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    Long,
}

/// One signal-model output (§8.2).
///
/// An output whose id, version, **and** content hash are not all the pinned triple is ignored and
/// **counts as missing** (§8.1, DEC-67, DEC-130 item 11), so it lowers the buy conviction and can
/// never raise it. An output whose `expires_at` is at or before its `as_of` needs no refusal: it is
/// simply never fresh, which is the freshness rule's own answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOutput {
    pub model_id: ModelId,
    pub model_version: ModelVersion,
    pub content_hash: Digest,
    pub instrument: AssetId,
    pub as_of: UtcNanos,
    pub expires_at: UtcNanos,
    pub direction: Direction,
    pub conviction: Conviction,
    pub confidence: Unit,
}

/// §8.3 step 1's three figures and the models they came from.
///
/// All three are computed and reported **before** any hold, so an outage is visible as an outage on
/// the journal and the approval screen rather than as a blank: with no fresh output at all the
/// answer is c = 0, b = −1, s = 0 (MC-B20, DEC-130 item 9).
///
/// `outputs_used` is a [`BTreeSet`], so it is sorted as the cases expect and two runs cannot differ
/// by iteration order (ES-21, DEC-130 item 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combined {
    pub outputs_used: BTreeSet<ModelId>,
    /// c = round₁₂(F ÷ W). A model without a fresh output counts as 0, so an outage never forces a
    /// sell.
    pub exit_conviction: Conviction,
    /// b = round₁₂((F − M) ÷ W). A model without a fresh output counts as fully bearish, so an
    /// outage never enlarges a buy (MI-10).
    pub buy_conviction: Conviction,
    /// s = round₁₂(Σ fresh wᵢ · confidenceᵢ ÷ W). The combined model score, **not** a probability
    /// of profit (§6.3, §6.4).
    pub score: Unit,
}

/// §8.3 step 1 over the outputs the caller has, at the instant the caller states.
///
/// **Fresh** is `as_of ≤ now < expires_at` **and** `now − as_of ≤ max_output_age_s` (§8.2); each
/// bound is exclusive or inclusive exactly as written. Only the latest fresh output per model
/// counts — latest `as_of`, ties by the index in `outputs`, which the caller supplies in journal
/// `seq` order because this crate cannot verify a sequence number it is handed (DEC-130 item 10).
///
/// W is the sum over **all** of `models`, never over the fresh ones: that is what makes a missing
/// model lower the buy conviction instead of silently rescaling the rest.
pub fn combine(
    models: &[SignalModel],
    outputs: &[ModelOutput],
    now: UtcNanos,
) -> Result<Combined, BuilderError> {
    let _ = (models, outputs, now);
    Err(BuilderError::Unimplemented)
}

/// The order builder's method and thresholds (§8.3), typed.
///
/// `mandate_spec::document::Sizing` is the same block as schema text; this is the view §8.3
/// computes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sizing {
    pub method: SizingMethod,
    pub entry_threshold: SizeFraction,
    pub exit_threshold: SizeFraction,
    pub rebalance_band: SizeFraction,
}

/// The four §5.3 limits the proposal is clipped to, typed.
///
/// Because the proposal is already clipped to them, the gate's §5.3 checks cannot deny an allowed
/// proposal for a limit the builder owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub max_position_usd: Usd,
    pub max_position_fraction: SizeFraction,
    pub max_order_usd: Usd,
    pub max_gross_exposure_usd: Usd,
}

/// An `accumulate` goal's bounds (§3.1, §8.3 step 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccumulateGoal {
    pub instrument: AssetId,
    pub target_qty: Qty,
    pub max_avg_price: Option<Price>,
    pub max_spend_usd: Usd,
}

/// What the agent is for, as far as §8.3 is concerned (§3.1).
///
/// `accumulate` disables discretionary exits and clips buys; `profit_stop` and `continuous` do
/// neither, and `protection` is the executor's, not the builder's (DEC-130 item 18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalKind {
    Continuous,
    ProfitStop,
    Accumulate(AccumulateGoal),
}

/// The §8 view of the mandate: what the builder reads, not the whole document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderMandate {
    pub models: Vec<SignalModel>,
    pub sizing: Sizing,
    pub limits: Limits,
    pub goal: GoalKind,
}

/// The account facts §8.3 sizes against.
///
/// `risk_mark` is its own input and not the quote: §8.3 step 3 says "MV (at the risk mark)" and
/// trading spec §8.2 makes the risk mark the bid for a long, which is why the reference cases
/// supply the two as one number. Taking it separately means a stale or non-sane mark can never be
/// silently replaced by a quote (DEC-130 item 6).
///
/// Every field is required. `gross_usd`, `working_opening_cost`, `goal_spent_usd` and `cost_basis`
/// are figures the caller holds, and a zero that was never stated is not the same as a zero that
/// was (DEC-85, DEC-130 item 19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSnapshot {
    pub agent_equity: Usd,
    pub position_qty: Qty,
    pub cost_basis: CostBasis,
    pub risk_mark: MarkPrice,
    pub gross_usd: Usd,
    /// The maximum cost of the agent's working **opening** orders in this instrument.
    pub working_opening_cost: Usd,
    pub goal_spent_usd: Usd,
}

/// The market the order would go to.
///
/// `increment` is a [`Qty`] and not a [`ShareIncrement`](mandate_num::ShareIncrement): the mandate
/// reference cases state `1`, `0.0001` and `0.000001`, and `MC-B28`'s expected `0.010025` is a
/// 6-place truncation that the nine places of `ShareIncrement::Fractional` would leave at
/// `0.010025062`. This is the correction DEC-128 item 27 made for a goal's increment, reached again
/// here by its own cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    pub instrument: AssetId,
    pub asset_class: AssetClass,
    pub session: MarketSession,
    /// Trading spec §9.6's close window, in which an equity discretionary exit goes out as a
    /// marketable limit order (DEC-70).
    pub in_close_window: bool,
    pub bid: Price,
    pub ask: Price,
    pub increment: Qty,
    pub min_order_usd: Usd,
    pub fee_rate_cash: FeeRate,
    pub fee_rate_asset: FeeRate,
}

/// Risk state and thesis facts the builder **reads** and never computes (§5.2, §5.5, §8.4, §8.5).
///
/// `size_factor` is the product of the active `scale_sizes` rungs, which is `mandate-risk`'s;
/// `new_instrument` and `thesis_confidence` are the research agent's, which is `mandate-research`'s
/// (DEC-132). This crate only multiplies by the one and reports the other two to a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskContext {
    pub size_factor: SizeFraction,
    pub drawdown: Unit,
    pub daily_pnl_fraction: mandate_num::Signed,
    pub position_pnl_fraction: mandate_num::Signed,
    pub bought_today_usd: Usd,
    /// Whether the agent has ever filled in this instrument. Not inferred from the position: a
    /// re-entry after a round trip is not a first trade (MC-B25, DEC-85).
    pub has_prior_fill: bool,
    pub new_instrument: bool,
    pub thesis_confidence: Unit,
}

/// Why the builder proposed nothing.
///
/// Declaration order is **not** evaluation order. The order the reasons are reached in is fixed by
/// DEC-130 item 21 where §8.3 leaves it implicit: in step 2 the `accumulate` check comes **before**
/// the flat-position check, so a flat `accumulate` agent under c ≤ −`exit_threshold` holds
/// [`HoldReason::DiscretionaryExitsDisabled`] and never [`HoldReason::NoPosition`] — the goal
/// disabled the exit, and reporting the position instead would read as though a position would have
/// been sold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HoldReason {
    NoFreshOutputs,
    NoPosition,
    DiscretionaryExitsDisabled,
    BetweenThresholds,
    AtOrAboveTarget,
    WithinRebalanceBand,
    BelowBandAfterClipping,
    WouldExceedMaxAvgPrice,
    BelowMinimumAfterClipping,
}

impl HoldReason {
    /// The form the reference cases write.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoFreshOutputs => "no_fresh_outputs",
            Self::NoPosition => "no_position",
            Self::DiscretionaryExitsDisabled => "discretionary_exits_disabled",
            Self::BetweenThresholds => "between_thresholds",
            Self::AtOrAboveTarget => "at_or_above_target",
            Self::WithinRebalanceBand => "within_rebalance_band",
            Self::BelowBandAfterClipping => "below_band_after_clipping",
            Self::WouldExceedMaxAvgPrice => "would_exceed_max_avg_price",
            Self::BelowMinimumAfterClipping => "below_minimum_after_clipping",
        }
    }
}

/// Which bound cut a proposal down (§8.3 steps 3 and 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Clip {
    Limits,
    Goal,
}

impl Clip {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Limits => "limits",
            Self::Goal => "goal",
        }
    }
}

/// How the order rests. Both are limit orders: v1 places no market orders (trading spec §5.1).
///
/// [`OrderShape::MarketableLimit`] is an equity discretionary exit inside the close window
/// (§9.6, DEC-70). Crypto has no regular session and no close window, so a crypto exit is never
/// marketable for this reason. The collar and the tick are the gate's and the executor's; applying
/// them here would price an order twice (DEC-130 item 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrderShape {
    Limit,
    MarketableLimit,
}

impl OrderShape {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Limit => "limit",
            Self::MarketableLimit => "marketable_limit",
        }
    }
}

/// The figures §8.3 reports, exact.
///
/// `cap` has no stated rounding (§5.2: comparisons are exact), and `target_value` and `delta` are
/// reported at 12 places by the caller that journals them. All four are [`UsdExact`] because cap
/// alone reaches 33 fractional places and the target 57, which neither [`Usd`] nor
/// [`Ratio`](mandate_num::Ratio) can carry; see the tests PR's Decisions needed for the ES-04 note
/// this widens.
///
/// `current_mv` is [`UsdExact`] too, which the brief's data shapes had as a [`Usd`]. A 9-place
/// quantity at a 12-place mark carries 21 fractional places, and the digit budget's 10¹² value
/// ceiling puts 12 digits in front of them: 33 significant digits, where `Usd` holds 28 places on a
/// 96-bit significand. A throwaway implementation run against
/// [`no_input_within_the_stated_bounds_overflows`] returned `overflow` on exactly that input, which
/// is what found it.
///
/// `target_value` and `delta` are `None` for a hold reached before step 2 computed them: no fresh
/// output, an exit, disabled exits, no position, or a conviction between the thresholds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sizes {
    pub cap: UsdExact,
    pub current_mv: UsdExact,
    pub target_value: Option<UsdExact>,
    pub delta: Option<UsdExact>,
}

/// What the builder proposes.
///
/// The `purpose` on a [`Action::Buy`] or [`Action::Sell`] is a **label**: the gate assigns purpose
/// from side and position and re-derives it (§6.1, DEC-130 item 2). A buy with no position is
/// labelled `open` and with one `increase`; a signal exit is `discretionary_exit`.
///
/// There is no sell above the position and no proposal crosses zero (DEC-32, §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Hold {
        reason: HoldReason,
    },
    Sell {
        purpose: Purpose,
        qty: Qty,
        limit_price: Price,
        order_usd: Usd,
        shape: OrderShape,
    },
    Buy {
        purpose: Purpose,
        qty: Qty,
        limit_price: Price,
        order_usd: Usd,
        /// The facts §6.2 step 4 evaluates the rules against, computed here because the exposure
        /// fields are the order's **after** values.
        action: ActionContext,
    },
}

/// One evaluation's answer: the action, the three combined figures, the reported sizes, and which
/// bounds clipped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub action: Action,
    pub combined: Combined,
    pub sizes: Sizes,
    pub clipped_by: BTreeSet<Clip>,
}

/// §8.3 in full: combine, decide, size, clip to the goal, and refuse anything below the minimum
/// order.
///
/// Pure: no clock, no I/O, no state between calls, and identical inputs give an identical proposal
/// (ES-21). A crossed quote is [`BuilderError::CrossedQuote`]; an input too wide for the exact
/// chain is `too_precise` or `overflow` and nothing is proposed (DEC-130 item 8).
///
/// The step-5 guard is `n ≤ 0` **or** a value below the minimum order, not the minimum alone, so a
/// zero-quantity buy is impossible even where `min_order_usd` and `rebalance_band` are both zero
/// (DEC-130 item 21).
pub fn propose(
    mandate: &BuilderMandate,
    account: &AccountSnapshot,
    market: &Market,
    risk: &RiskContext,
    outputs: &[ModelOutput],
    now: UtcNanos,
) -> Result<Proposal, BuilderError> {
    let _ = (mandate, account, market, risk, outputs, now);
    Err(BuilderError::Unimplemented)
}
