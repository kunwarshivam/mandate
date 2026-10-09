#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The vocabulary shared by every crate that reads a mandate (ADR-0001 ES-02 layer 1;
//! [task brief](../../../docs/project/tasks/M5-F-mandate-spec.md), DEC-128 items 1 and 21).
//!
//! These types name things, they do not decide anything: no rule, no limit, and no arithmetic lives
//! here. That is the point of the layer. `mandate-spec` (3) defines the mandate document and its
//! rules, `mandate-risk` (4) the gate, `mandate-builder` (5) the order builder, and each of them
//! needs to say "us equity", "this instrument", "exits only" without depending on the others.
//!
//! Nothing here reads a clock, allocates a random value, or iterates a hash map (ES-21). Every set
//! is a [`BTreeSet`](std::collections::BTreeSet), so a fold replays in one order.

use std::collections::BTreeSet;

mod profile;

pub use profile::{
    CapabilityProfile, Cell, Idempotency, OrderType, ProfileError, ProtectionForm, QuantityForm,
    Retry, Row, TimeInForce,
};

/// An asset class the platform trades (mandate spec §3, trading-domain spec §3).
///
/// The text form is the schema's (`us_equity`, `crypto`), which is also the reference cases' and the
/// journal's. `mandate-marketdata` keeps its own vendor spelling (`us-equity`) for dataset paths;
/// that is a URL segment, not this name (DEC-128 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetClass {
    Crypto,
    UsEquity,
}

impl AssetClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crypto => "crypto",
            Self::UsEquity => "us_equity",
        }
    }

    /// The schema form, or [`DomainError::UnknownAssetClass`].
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        match text {
            "crypto" => Ok(Self::Crypto),
            "us_equity" => Ok(Self::UsEquity),
            _ => Err(DomainError::UnknownAssetClass),
        }
    }
}

impl core::fmt::Display for AssetClass {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An instrument's identity as a mandate, a thesis, or a reference case names it: the schema's
/// `$defs/uuid`, 36 lowercase hexadecimal characters and hyphens (`asset_id`, `instrument_id`).
///
/// This is the narrower of the workspace's two instrument identifiers. `mandate_accounting::InstrumentId`
/// is the broker-facing one and accepts any non-empty string, so [`AssetId`] converts into it and
/// never the other way without a check (DEC-128 item 21).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(String);

impl AssetId {
    /// Exactly the schema's `uuid` form: `8-4-4-4-12` lowercase hexadecimal, hyphens where the schema
    /// puts them and nowhere else. Uppercase is rejected rather than folded, so two spellings of one
    /// id can never both be in a working universe.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
        let mut groups = text.split('-');
        for width in GROUPS {
            let group = groups.next().ok_or(DomainError::MalformedAssetId)?;
            if group.len() != width
                || !group
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(DomainError::MalformedAssetId);
            }
        }
        if groups.next().is_some() {
            return Err(DomainError::MalformedAssetId);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for AssetId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The set of instruments an agent may open or increase now (mandate spec §2.3).
///
/// It is runtime state, folded from the account stream's `UniverseChanged` events, never part of the
/// hashed document (§3.2). [`WorkingUniverse::Unavailable`] is a state of its own and not an empty
/// [`WorkingUniverse::Known`]: a gate that has not read the fold must deny every opening (§5.3 check
/// 2) and must say *why* it denied, and "not read yet" is a different answer for an operator than
/// "the agent holds nothing". An empty `Known` denies too, and reports that instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkingUniverse {
    Known {
        instruments: BTreeSet<AssetId>,
        /// True under bring-your-own-strategy, where the pinned universe *is* the working universe
        /// and admission adds nothing (§2.3, MI-20).
        pinned: bool,
    },
    Unavailable,
}

/// Which broker environment a mandate is bound to, immutable across its versions (§3, V-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Environment {
    Paper,
    Live,
}

/// An order's side. No short sales in v1, so a sell never exceeds the position (§6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Buy,
    Sell,
}

/// Why an order exists, **assigned by the gate** from the side and the position and never taken from
/// the proposer's label (§6.1).
///
/// The ordering is not a severity: it is the declaration order, and every variant except
/// [`Purpose::Open`] and [`Purpose::Increase`] is built-in AUTO and is never denied (§6.2 step 3,
/// MI-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Purpose {
    Open,
    Increase,
    DiscretionaryExit,
    OwnerExit,
    RiskExit,
    Protective,
}

impl Purpose {
    /// True for the four purposes §6.2 step 3 makes built-in AUTO, which the gate never denies.
    pub fn reduces_risk(self) -> bool {
        matches!(
            self,
            Self::DiscretionaryExit | Self::OwnerExit | Self::RiskExit | Self::Protective
        )
    }
}

/// What the autonomy rules decide for an action (§6.2).
///
/// [`Ord`] **is** the strictness order — `Auto < Ask < Deny` — because §6.2 step 5 takes the
/// stricter of the rule result and `autonomy.admission`, and §9.2 calls an autonomy change reducing
/// only when every decision becomes at least as strict. Deriving the order from the declaration
/// makes "stricter" one thing in one place (MI-11, MI-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AutonomyDecision {
    Auto,
    Ask,
    Deny,
}

impl AutonomyDecision {
    /// The stricter of two decisions: §6.2 step 5's admission ceiling, and §9.2's comparison.
    ///
    /// A maximum over the declaration order, not a hand-written comparison, so it cannot disagree with
    /// [`Ord`] about which of two decisions is stricter.
    pub fn stricter(self, other: Self) -> Self {
        self.max(other)
    }
}

/// An agent's effective mode: the strictest active restriction (§5.9).
///
/// [`Ord`] is the severity order `Normal < ExitsOnly < Paused < Stopped`, so the effective mode is a
/// maximum and MI-6 is a property of the type rather than of a comparison written by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentMode {
    Normal,
    ExitsOnly,
    Paused,
    Stopped,
}

/// A market session, as a bar or a risk input labels it (trading-domain spec §4.3).
///
/// `Crypto` is the continuous session; equities never carry it, and nothing ever fills in
/// `Overnight` (DEC-30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarketSession {
    Overnight,
    PreMarket,
    Regular,
    AfterHours,
    Crypto,
}

impl MarketSession {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overnight => "overnight",
            Self::PreMarket => "pre_market",
            Self::Regular => "regular",
            Self::AfterHours => "after_hours",
            Self::Crypto => "crypto",
        }
    }

    /// The form the §6.3 `session` condition field and the reference cases use, which omits
    /// `overnight` because no order may trade there.
    pub fn parse_condition_form(text: &str) -> Result<Self, DomainError> {
        match text {
            "pre_market" => Ok(Self::PreMarket),
            "regular" => Ok(Self::Regular),
            "after_hours" => Ok(Self::AfterHours),
            "crypto" => Ok(Self::Crypto),
            _ => Err(DomainError::UnknownSession),
        }
    }
}

impl core::fmt::Display for MarketSession {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why mandate spec §8.5 refuses a thesis: its seventeen ordered checks, each named by the reason a
/// refusal journals (journal spec §9.4's `reason`).
///
/// This is the one home of the list (DEC-415). `mandate-research` decides the checks,
/// `mandate-journal` types a thesis record's `reason` with [`ThesisRefusal::CODES`], and
/// `mandate-spec` re-derives the mandate's checks from a stored record; each reads this type, so the
/// three cannot disagree about a code or an order.
///
/// The declaration order **is** the check order, so [`Ord`] sorts by check and the first failing
/// check is a minimum. A check is named by its number from 1, as §8.5 numbers it, and nowhere by a
/// position from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThesisRefusal {
    /// Check 1: `direction` is not `long` (DEC-32); an ignored output.
    DirectionNotAllowed,
    /// Check 2: `expires_at` is not `as_of + horizon_s` (§8.2); an ignored output.
    HorizonMismatch,
    /// Check 3: `predecessor_thesis_id` is not present exactly when `revision > 0`; an ignored output.
    RevisionWithoutPredecessor,
    /// Check 4: no signal model has `admits_instruments`, or `behavior.research` is unset (V-036).
    ResearchDisabled,
    /// Check 5: `universe.pinned` is true (MI-20).
    UniversePinned,
    /// Check 6: `autonomy.admission` is `deny` (§6.2).
    AdmissionDenied,
    /// Check 7: the day's research spend has reached `cost_cap_usd_per_day` (DEC-120).
    CostCapReached,
    /// Check 8: a profile pins a data universe the instrument is not in (DEC-103).
    NotInDataUniverse,
    /// Check 9: the instrument is under an operator per-thesis halt (DEC-100).
    OperatorHalt,
    /// Check 10: the instrument's asset class is not in `universe.asset_classes`.
    NotAllowedAssetClass,
    /// Check 11: a leveraged or inverse ETP without `universe.leveraged_etps_enabled` and its disclosure (V-005).
    LeveragedEtpNotEnabled,
    /// Check 12: the instrument fails the eligibility floor (trading spec §3.2).
    EligibilityFloor,
    /// Check 13: another agent on the account claims the instrument's group (trading spec §7.1).
    InstrumentGroupClaimed,
    /// Check 14: a cited evidence source is not on the allowlist (DEC-101).
    SourceNotAllowlisted,
    /// Check 15: neither an independent source nor market data corroborates the thesis (DEC-101).
    NoCorroboration,
    /// Check 16: the lineage is retired, or `revision` exceeds `max_revisions_per_lineage` (DEC-111).
    LineageRetired,
    /// Check 17: the working universe already holds at least `universe.max_instruments` instruments (MI-15); the only check a renewal skips.
    UniverseFull,
}

impl ThesisRefusal {
    /// Every reason's code, in check order: the closed set journal spec §9.4 accepts at `reason`.
    pub const CODES: [&'static str; 17] = [
        Self::DirectionNotAllowed.code(),
        Self::HorizonMismatch.code(),
        Self::RevisionWithoutPredecessor.code(),
        Self::ResearchDisabled.code(),
        Self::UniversePinned.code(),
        Self::AdmissionDenied.code(),
        Self::CostCapReached.code(),
        Self::NotInDataUniverse.code(),
        Self::OperatorHalt.code(),
        Self::NotAllowedAssetClass.code(),
        Self::LeveragedEtpNotEnabled.code(),
        Self::EligibilityFloor.code(),
        Self::InstrumentGroupClaimed.code(),
        Self::SourceNotAllowlisted.code(),
        Self::NoCorroboration.code(),
        Self::LineageRetired.code(),
        Self::UniverseFull.code(),
    ];

    /// Every reason, in check order.
    pub const fn all() -> [Self; 17] {
        [
            Self::DirectionNotAllowed,
            Self::HorizonMismatch,
            Self::RevisionWithoutPredecessor,
            Self::ResearchDisabled,
            Self::UniversePinned,
            Self::AdmissionDenied,
            Self::CostCapReached,
            Self::NotInDataUniverse,
            Self::OperatorHalt,
            Self::NotAllowedAssetClass,
            Self::LeveragedEtpNotEnabled,
            Self::EligibilityFloor,
            Self::InstrumentGroupClaimed,
            Self::SourceNotAllowlisted,
            Self::NoCorroboration,
            Self::LineageRetired,
            Self::UniverseFull,
        ]
    }

    /// The journaled reason (ADR-0001 ES-09), the identifier §8.5's table states.
    pub const fn code(self) -> &'static str {
        match self {
            Self::DirectionNotAllowed => "direction_not_allowed",
            Self::HorizonMismatch => "horizon_mismatch",
            Self::RevisionWithoutPredecessor => "revision_without_predecessor",
            Self::ResearchDisabled => "research_disabled",
            Self::UniversePinned => "universe_pinned",
            Self::AdmissionDenied => "admission_denied",
            Self::CostCapReached => "cost_cap_reached",
            Self::NotInDataUniverse => "not_in_data_universe",
            Self::OperatorHalt => "operator_halt",
            Self::NotAllowedAssetClass => "not_allowed_asset_class",
            Self::LeveragedEtpNotEnabled => "leveraged_etp_not_enabled",
            Self::EligibilityFloor => "eligibility_floor",
            Self::InstrumentGroupClaimed => "instrument_group_claimed",
            Self::SourceNotAllowlisted => "source_not_allowlisted",
            Self::NoCorroboration => "no_corroboration",
            Self::LineageRetired => "lineage_retired",
            Self::UniverseFull => "universe_full",
        }
    }

    /// The §8.5 check number, 1 to 17.
    pub const fn check_number(self) -> u8 {
        match self {
            Self::DirectionNotAllowed => 1,
            Self::HorizonMismatch => 2,
            Self::RevisionWithoutPredecessor => 3,
            Self::ResearchDisabled => 4,
            Self::UniversePinned => 5,
            Self::AdmissionDenied => 6,
            Self::CostCapReached => 7,
            Self::NotInDataUniverse => 8,
            Self::OperatorHalt => 9,
            Self::NotAllowedAssetClass => 10,
            Self::LeveragedEtpNotEnabled => 11,
            Self::EligibilityFloor => 12,
            Self::InstrumentGroupClaimed => 13,
            Self::SourceNotAllowlisted => 14,
            Self::NoCorroboration => 15,
            Self::LineageRetired => 16,
            Self::UniverseFull => 17,
        }
    }

    /// §8.2's ignored outputs: checks 1 to 3 are refusals that also mark the model output ignored.
    pub const fn is_ignored_output(self) -> bool {
        matches!(
            self,
            Self::DirectionNotAllowed | Self::HorizonMismatch | Self::RevisionWithoutPredecessor
        )
    }

    /// The reason a code names, or [`DomainError::UnknownThesisRefusal`]. Exact spelling only; the
    /// codes are spelled once, in [`ThesisRefusal::code`].
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        Self::all()
            .into_iter()
            .find(|reason| reason.code() == text)
            .ok_or(DomainError::UnknownThesisRefusal)
    }
}

impl core::fmt::Display for ThesisRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("not an asset class this platform trades")]
    UnknownAssetClass,
    #[error("not a session an order may trade in")]
    UnknownSession,
    #[error("not the schema's lowercase uuid form")]
    MalformedAssetId,
    #[error("not a reason mandate spec §8.5 refuses a thesis for")]
    UnknownThesisRefusal,
    /// The stubs of this story's tests PR return this, so every pending test fails on them
    /// (DEC-77, DEC-83); the implementation PR replaces the stubs and removes the variant.
    #[error("this part of the shared vocabulary is not implemented yet")]
    Unimplemented,
}

impl DomainError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::UnknownAssetClass => "unknown_asset_class",
            Self::UnknownSession => "unknown_session",
            Self::MalformedAssetId => "malformed_asset_id",
            Self::UnknownThesisRefusal => "unknown_thesis_refusal",
            Self::Unimplemented => "unimplemented",
        }
    }
}
