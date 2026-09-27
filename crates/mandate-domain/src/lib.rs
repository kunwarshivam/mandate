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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("not an asset class this platform trades")]
    UnknownAssetClass,
    #[error("not a session an order may trade in")]
    UnknownSession,
    #[error("not the schema's lowercase uuid form")]
    MalformedAssetId,
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
            Self::Unimplemented => "unimplemented",
        }
    }
}
