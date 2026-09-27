//! The minimal stream-F shapes this crate needs before `mandate-spec` and `mandate-domain` exist.
//!
//! Every name here is the one stream F's brief fixes (DEC-128 item 21), so nothing renames when
//! those crates land: [`WorkingUniverse`] is `mandate-domain`'s, over [`AssetId`]; [`RiskSnapshot`]
//! carries `day_start_equity`, `latched`, `active_rungs` and `inherited_loss`; [`ValidatedMandate`]
//! is the only way a gate can be handed a mandate.
//!
//! This module is temporary by construction. The first `mandate-risk` implementation PR after
//! stream F's tests PR merges **deletes it** and takes the real types; nothing here is a second
//! design, only a subset of fields the mandate schema already names.

use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{Fraction, Ratio, Usd};

/// The schema's `$defs/uuid` id, which every mandate rule and every reference case names.
/// `mandate_accounting::InstrumentId` stays the wider broker-facing id (DEC-128 item 21).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetId(String);

impl AssetId {
    /// # Errors
    /// Returns [`SpecTypeError::EmptyAssetId`] for an empty id.
    pub fn new(id: &str) -> Result<Self, SpecTypeError> {
        if id.is_empty() {
            return Err(SpecTypeError::EmptyAssetId);
        }
        Ok(Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpecTypeError {
    #[error("an asset id is never empty")]
    EmptyAssetId,
}

/// The set of instruments the agent may open or increase now (mandate spec §2.3).
///
/// [`WorkingUniverse::Unavailable`] is a real state and not an empty set: check 2 must refuse to
/// decide while the fold has not been read, where an empty `Known` denies and says "universe
/// empty". There is no `Default` and no constructor that guesses, so "we did not read it" cannot be
/// mistaken for "nothing is admitted" (DEC-129 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkingUniverse {
    Known {
        instruments: BTreeSet<AssetId>,
        pinned: bool,
    },
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentMode {
    Normal,
    ExitsOnly,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstrumentRestriction {
    StaleMark,
    RemovedInstrument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LimitKey {
    MaxDailyLoss,
    DrawdownRung(u8),
    LifetimeFloor,
    ProfitStop,
}

/// Stream F's `Snapshot`, narrowed to the fields the gate reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskSnapshot {
    pub agent_equity: Usd,
    pub high_water_mark: Usd,
    pub day_start_equity: Usd,
    pub capital_base: Usd,
    pub inherited_loss: Usd,
    pub latched: BTreeSet<LimitKey>,
    /// Each active rung and how long it has been active, in whole seconds, which §5.5's
    /// `breach_confirm_s` trim guard needs. **Stream F's shape**: a map, not a set beside a second
    /// map, so there is no way to hold a rung that is active with no duration or the reverse (the
    /// coordinator's ruling on #136 review round 2).
    pub active_rungs: BTreeMap<u8, u64>,
    /// The product of the active `scale_sizes` rungs' factors. **Folded by stream F**, not computed
    /// here: it is risk state, and F's `Snapshot` already carries it, so computing it in two crates
    /// would let them disagree. `Ratio` is F's type, taken so the implementation PR that deletes
    /// this module needs no conversion.
    pub size_factor: Ratio,
    pub agent_mode: AgentMode,
}

/// The mandate's risk block, which is all of the document the gate reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskLimits {
    pub max_position_usd: Usd,
    pub max_position_fraction: Fraction,
    pub max_order_usd: Usd,
    pub max_gross_exposure_usd: Usd,
    pub max_orders_per_day: u32,
    pub reentry_cooldown_s: u32,
    pub rebalance_band: Fraction,
    /// How long a rung must have been active before a `trim_to_target` trim is proposed
    /// (mandate §5.5, and §5.5's rung table). A mandate limit like the rest, read from the
    /// mandate rather than configured, so a rung that has only just breached cannot trim.
    pub breach_confirm_s: u32,
    pub drawdown_ladder: Vec<Rung>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rung {
    pub index: u8,
    pub at: Fraction,
    pub action: RungAction,
    pub factor: Option<Fraction>,
    pub scale_action: Option<ScaleAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RungAction {
    ScaleSizes,
    ExitsOnly,
    FlattenAndPause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleAction {
    LimitBuys,
    TrimToTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalState {
    Running,
    Holding,
}

/// A mandate no one validated cannot reach the gate: this is the only constructor, and stream F's
/// `ValidatedMandate::new` replaces it when `mandate-spec` lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedMandate {
    risk: RiskLimits,
    goal_state: GoalState,
    leveraged_etps_enabled: bool,
    leveraged_etp_disclosure_accepted: bool,
}

impl ValidatedMandate {
    #[must_use]
    pub fn from_validated_parts(
        risk: RiskLimits,
        goal_state: GoalState,
        leveraged_etps_enabled: bool,
        leveraged_etp_disclosure_accepted: bool,
    ) -> Self {
        Self {
            risk,
            goal_state,
            leveraged_etps_enabled,
            leveraged_etp_disclosure_accepted,
        }
    }

    #[must_use]
    pub fn risk(&self) -> &RiskLimits {
        &self.risk
    }

    #[must_use]
    pub fn goal_state(&self) -> GoalState {
        self.goal_state
    }

    #[must_use]
    pub fn leveraged_etps_enabled(&self) -> bool {
        self.leveraged_etps_enabled
    }

    #[must_use]
    pub fn leveraged_etp_disclosure_accepted(&self) -> bool {
        self.leveraged_etp_disclosure_accepted
    }
}
