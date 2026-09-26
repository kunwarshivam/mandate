//! The `conviction_linear` order builder ([mandate spec §8.3]) and the §6.2 composition that turns a
//! proposal plus a gate verdict into an outcome.
//!
//! [mandate spec §8.3]: ../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60

use std::collections::BTreeSet;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{
    CostBasis, FeeRate, MarkPrice, Price, Qty, ShareIncrement, Signed, SizeFraction, Unit, Usd,
    UsdExact,
};
use mandate_time::UtcNanos;

use crate::BuilderError;
use crate::combine::{Combined, ModelOutput, SignalModel};
use crate::policy::{ActionContext, Autonomy, AutonomyPolicy, Purpose, Session};

/// v1 has one sizing method, shown in plain language on the confirmation screen (spec §8.3). One
/// variant, so a method this crate does not implement is unrepresentable rather than rejected, as
/// [`Direction`](crate::Direction) is (DEC-130 item 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizingMethod {
    ConvictionLinear,
}

/// `behavior.sizing` (spec §3): the owner's thresholds and band, all envelope fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizing {
    pub method: SizingMethod,
    pub entry_threshold: SizeFraction,
    pub exit_threshold: SizeFraction,
    pub rebalance_band: SizeFraction,
}

/// The `risk.*` limits §8.3 clips a proposal to (spec §5.3). The builder clips to them so the gate's
/// §5.3 checks cannot deny a proposal for a limit the builder itself owns; the gate still re-checks
/// every one of them, because it is the enforcement point (`AGENTS.md` rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_position_usd: Usd,
    pub max_position_fraction: SizeFraction,
    pub max_order_usd: Usd,
    pub max_gross_exposure_usd: Usd,
}

/// An `accumulate` goal's parameters (spec §3.1). Its universe is pinned to the goal instrument and
/// discretionary exits are disabled, so a negative conviction holds rather than selling (`MC-B29`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccumulateGoal {
    pub instrument: InstrumentId,
    pub target_qty: Qty,
    pub max_avg_price: Option<Price>,
    pub max_spend_usd: Usd,
}

/// Which goal the agent runs (spec §3.1). Only `accumulate` changes what §8.3 does: it disables the
/// discretionary exit and clips a buy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalKind {
    Continuous,
    ProfitStop,
    Accumulate(AccumulateGoal),
}

/// The §8 fields of a mandate, and nothing else (DEC-130 item 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderMandate {
    pub models: Vec<SignalModel>,
    pub sizing: Sizing,
    pub limits: Limits,
    pub goal: GoalKind,
}

/// What the account ledger and the risk state say about the instrument being sized (spec §5.1,
/// §8.3 step 3).
///
/// `risk_mark` is its own input rather than the quote's bid: spec §8.3 step 3 sizes against "MV (at
/// the risk mark)" and trading spec §8.2 makes the risk mark the bid for a long, which is why the
/// reference cases supply the two as one number. Taking it separately means a stale or non-sane mark
/// can never be silently replaced by a quote (DEC-130 item 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountSnapshot {
    pub agent_equity: Usd,
    pub position_qty: Qty,
    pub cost_basis: CostBasis,
    pub risk_mark: MarkPrice,
    /// Σ |MV| of the agent's positions plus the max cost of its working opening orders, before this
    /// order (spec §5.3).
    pub gross_usd: Usd,
    /// Max cost of the agent's working opening orders in **this** instrument (spec §8.3 step 3).
    pub working_opening_cost: Usd,
    /// Σ of the agent's buy fills in the goal instrument, fees included; sales never reduce it
    /// (spec §3.1).
    pub goal_spent_usd: Usd,
}

/// The instrument and the market it would trade in. `bid` and `ask` price an exit and a buy exactly
/// as §8.3 does: neither is collared or tick-rounded here, because the collar (trading spec §9.6) and
/// the tick (§2.1) belong to the gate and the executor, and applying them twice would price an order
/// twice (DEC-130 item 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub session: Session,
    /// The last minutes of the regular session (trading spec §9.6, DEC-70), in which an equity
    /// discretionary exit goes out as a marketable limit order.
    pub in_close_window: bool,
    pub bid: Price,
    pub ask: Price,
    pub increment: ShareIncrement,
    pub min_order_usd: Usd,
    /// The cash fee rate an `accumulate` clip adds to the per-unit cost (spec §8.3 step 4).
    pub fee_rate_cash: FeeRate,
    /// The asset fee rate an `accumulate` clip takes off the quantity received (crypto fees are
    /// taken in the asset, spec §8.3 step 4).
    pub fee_rate_asset: FeeRate,
}

/// Risk state and thesis facts the builder **reads** and never computes (spec §5.2, §5.5, §8.4).
/// Every field is required; nothing is defaulted (DEC-130 item 19, DEC-85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskContext {
    /// The product of the active drawdown rungs' factors (spec §5.5); one when no rung is active.
    pub size_factor: SizeFraction,
    pub drawdown: Unit,
    pub daily_pnl_fraction: Signed,
    pub position_pnl_fraction: Signed,
    pub bought_today_usd: Usd,
    /// Whether the agent has any prior fill in this instrument. Never inferred from the position: a
    /// re-entry after a round trip is not a first trade, which is `MC-B25`.
    pub has_prior_fill: bool,
    /// Whether this order would be the first in an instrument the research agent admitted and the
    /// agent is flat in it (spec §6.3, ADR-0002 part 3). Stream J supplies it.
    pub new_instrument: bool,
    /// The admitting thesis's self-reported, uncalibrated confidence (spec §8.2); zero when no
    /// thesis applies.
    pub thesis_confidence: Unit,
}

/// Why the builder proposed nothing. Declaration order is **not** evaluation order: DEC-130 item 21
/// fixes that, and in step 2 the `accumulate` check precedes the flat-position check, so a flat
/// `accumulate` agent under a negative conviction reads `DiscretionaryExitsDisabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// What bound a buy, in the order §8.3 applies them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Clip {
    Limits,
    Goal,
}

/// How the exit reaches the market. In the close window an equity discretionary exit is marketable
/// (trading spec §9.6, DEC-70); everything else is a plain limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderShape {
    Limit,
    MarketableLimit,
}

/// The reported figures of §8.3, exact.
///
/// `cap` has no rounding in the spec and §5.2 says comparisons are exact, so it is reported exactly;
/// `target_value` and `delta` are present only once step 2 reaches them, and the caller that
/// journals them names the 12 places it reports them at (DEC-130 item 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    pub cap: UsdExact,
    pub current_mv: Usd,
    pub target_value: Option<UsdExact>,
    pub delta: Option<UsdExact>,
}

/// What the builder proposes. A `Buy` carries the [`ActionContext`] the autonomy rules read, built
/// from the same numbers that sized it, so the exposure fields are this order's **after** values.
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
        action: Box<ActionContext>,
    },
}

/// One evaluation's result: what to do, the figures behind it, and what bound it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub action: Action,
    pub combined: Combined,
    pub sizes: Sizes,
    pub clipped_by: BTreeSet<Clip>,
}

/// The risk gate's dry-run verdict (trading spec §9.1), which reaches [`decide`] as a **value**: the
/// builder never calls the gate, so nothing here can turn a verdict into an allow (DEC-130 item 2).
///
/// `Defer` is the gate's alone and applies to discretionary exits only — an equity exit outside the
/// regular session — and it never becomes a deny (DEC-48). [`propose`] cannot produce it: it takes a
/// [`Market`] and so knows the session, but the verdict is not its to give (DEC-130 item 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Allow,
    Deny,
    Defer,
}

/// What happens to a proposal after the dry run (spec §6.2 steps 2 to 6).
///
/// `Skipped` and `Deferred` are outcomes, not [`crate::Decision`] variants, so a gate verdict can
/// never be mistaken for an autonomy decision in a `match` (DEC-130 item 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The builder proposed nothing, so there is no order to gate or classify.
    NotProposed,
    /// The gate denied: the action is skipped and journaled, and **no approval is requested**
    /// (spec §6.2 step 2, DEC-05).
    Skipped,
    /// The gate deferred: nothing is submitted and nothing is stored. The builder proposes again at
    /// the next evaluation, so a deferred exit happens only if the signal still calls for it.
    Deferred,
    Classified(Autonomy),
}

/// Runs `conviction_linear` for one instrument at one evaluation (spec §8.3).
///
/// 1. **Combine** (step 1), through [`combine`](crate::combine).
/// 2. **Decide** (step 2): an exit at `exit_conviction ≤ −exit_threshold`, which sells the whole
///    position at the bid and is disabled for an `accumulate` goal; a buy at
///    `buy_conviction ≥ entry_threshold` toward a target of `buy_conviction × cap × size factor`,
///    where `cap = min(max_position_usd, max_position_fraction × E)`; otherwise a hold.
/// 3. **Size** (step 3): `Delta = T − MV at the risk mark − working opening cost`, held when it is
///    not positive (there are no signal trims in v1, so a positive conviction never sells) or below
///    `rebalance_band × cap`; otherwise the buy value is the least of Delta, `max_order_usd`, the cap
///    headroom, and the gross headroom, at the ask, truncated to the increment, and held again if
///    that value falls below the band (no tiny top-ups after clipping).
/// 4. **Accumulate clips** (step 4), with per-unit cost `a = ask × (1 + cash fee rate)` and quantity
///    received per unit `β = 1 − asset fee rate`, each bound truncated to the increment, and a hold
///    if the projected average would exceed `max_avg_price`.
/// 5. Held when the quantity is not positive or the value is below the minimum order (step 5).
///
/// Errors: `no_signal_models`; `weight_sum_zero`; `crossed_quote`; `accumulate_instrument_mismatch`;
/// and the arithmetic and time errors it wraps, which is how an input too precise for the types to
/// hold refuses the order (DEC-130 item 8).
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

/// Applies the gate's dry-run verdict and then the autonomy rules (spec §6.2 steps 2 to 6): a `Deny`
/// skips and asks nobody, a `Defer` stores nothing, and an `Allow` is classified by
/// [`classify`](crate::classify), and a hold is [`Outcome::NotProposed`] whatever the verdict says.
///
/// This is the only path to an [`Autonomy`], which is what makes "no approval is ever requested for
/// an action the gate would deny" hold by construction rather than by discipline (DEC-130 item 16).
pub fn decide(
    policy: &AutonomyPolicy,
    proposal: &Proposal,
    verdict: GateVerdict,
) -> Result<Outcome, BuilderError> {
    let _ = (policy, proposal, verdict);
    Err(BuilderError::Unimplemented)
}
