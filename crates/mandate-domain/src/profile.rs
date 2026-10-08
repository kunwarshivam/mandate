//! A broker's capability profile: what its published contract accepts, as data (DEC-531,
//! [ADR-0004](../../../docs/adr/0004-broker-capability-profiles.md), DEC-630).
//!
//! A connector declares one; the builder, protection and reconciliation read it, and never name a
//! broker or read the asset class to learn a broker rule. The platform's own policy (limit
//! openings in the regular session, no short sales) is never a row here: the builder intersects
//! the two.
//!
//! A profile is validated once, at [`CapabilityProfile::new`], and hashed over its canonical
//! object through `mandate-canon` (journal spec §4), so a broker rule that changes is a new hash.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value};

use crate::{AssetClass, MarketSession};

/// An order type, spelled as trading spec §5.2 spells it (`market`, `limit`, `stop`,
/// `stop_limit`). Robinhood's `stop_market` is [`OrderType::Stop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
}

/// How an order states its size: whole shares, a fractional quantity, or a dollar amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuantityForm {
    Whole,
    Fractional,
    Notional,
}

/// A time in force. [`TimeInForce::Day`] is every broker's order that expires at the end of the
/// session it was placed in: Alpaca's `day` and Robinhood's `gfd` alike (DEC-630 item 5). Each
/// connector maps its own spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TimeInForce {
    Day,
    Gtc,
    Ioc,
}

/// A form a protective exit can take at the broker. A profile that offers none lists none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProtectionForm {
    Bracket,
    Oco,
    /// One resting stop-limit for the whole position.
    StopLimit,
}

/// Whether re-sending an order with the same client order id can never create a second order.
///
/// [`Retry::Unknown`] is read exactly as [`Retry::NotIdempotent`]: never a blind re-send. A lost
/// answer is queried and otherwise stays `Unknown`, which blocks the instrument (DEC-160,
/// DEC-529, DEC-630 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Retry {
    Idempotent,
    NotIdempotent,
    Unknown,
}

impl Retry {
    /// Whether an order whose answer was lost may be sent again with the same client order id
    /// without first learning what became of it. Only [`Retry::Idempotent`] may: the broker then
    /// refuses the second order rather than taking it.
    pub fn may_resend_blindly(self) -> bool {
        matches!(self, Self::Idempotent)
    }
}

/// What the broker offers for an order of one type and one quantity form (DEC-630 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub order_type: OrderType,
    pub quantity_form: QuantityForm,
    pub times_in_force: BTreeSet<TimeInForce>,
    /// The protective forms an order of this type and quantity form may be sent as: a bracket's
    /// entry, an OCO's parent (its take-profit), or the one resting stop-limit. Never the forms it
    /// may only be a leg of; each form fixes its own legs (DEC-630 item 1).
    pub protection_forms: BTreeSet<ProtectionForm>,
}

/// One asset class in one session: every order type and quantity form it accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub asset_class: AssetClass,
    pub session: MarketSession,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Idempotency {
    /// Whether an order carries an id the client chooses.
    pub client_order_id: bool,
    pub retry: Retry,
    /// Whether orders can be looked up by that id.
    pub query_by_client_order_id: bool,
}

/// Where one cell sits: its row's asset class and session, then its order type and quantity
/// form.
type Place = (AssetClass, MarketSession, OrderType, QuantityForm);

/// A validated profile with its canonical object and that object's hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityProfile {
    canonical: Value,
    content_hash: Digest,
    idempotency: Idempotency,
    cells: BTreeMap<Place, Cell>,
}

impl CapabilityProfile {
    /// The `kind` member of the canonical object, and of its `ConfigSnapshotRegistered`.
    pub const KIND: &'static str = "broker_profile";

    /// Validates the rows and the idempotency claims, and builds the canonical object. The order
    /// of `rows` and of each row's `cells` is not part of the profile.
    pub fn new(
        _profile_version: u32,
        _rows: Vec<Row>,
        _idempotency: Idempotency,
    ) -> Result<Self, ProfileError> {
        Err(ProfileError::Unimplemented)
    }

    /// The canonical object (DEC-630 item 6) that [`CapabilityProfile::content_hash`] hashes.
    pub fn canonical(&self) -> &Value {
        &self.canonical
    }

    /// SHA-256 of the canonical bytes of [`CapabilityProfile::canonical`].
    pub fn content_hash(&self) -> Digest {
        self.content_hash
    }

    /// The broker's idempotency members. A re-send reads [`Retry::may_resend_blindly`], never
    /// the variant itself.
    pub fn idempotency(&self) -> Idempotency {
        self.idempotency
    }

    /// The one cell for an order of this type and quantity form in this asset class and session.
    /// An order the profile does not list is refused [`ProfileError::NotOffered`], so a missing
    /// cell is never read as "anything goes" (DEC-630 item 9).
    pub fn cell(
        &self,
        asset_class: AssetClass,
        session: MarketSession,
        order_type: OrderType,
        quantity_form: QuantityForm,
    ) -> Result<&Cell, ProfileError> {
        let _ = (&self.cells, asset_class, session, order_type, quantity_form);
        Err(ProfileError::Unimplemented)
    }
}

/// Why a declared profile is refused, or a read of it finds no cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    /// The stubs of this story's tests PR return it (DEC-77). Like `DomainError::Unimplemented`,
    /// the variant outlives them, so the tests stay unchanged by the implementation.
    #[error("the capability profile is not implemented yet")]
    Unimplemented,
    #[error("a profile's version starts at 1")]
    VersionZero,
    #[error("a profile lists at least one row")]
    NoRows,
    #[error("two rows name the same asset class and session")]
    DuplicateRow,
    #[error("a row lists at least one cell")]
    EmptyRow,
    #[error("two cells of a row name the same order type and quantity form")]
    DuplicateCell,
    #[error("a cell lists at least one time in force")]
    NoTimeInForce,
    #[error("only a broker with a client order id can deduplicate or be queried by it")]
    ClaimWithoutClientId,
    #[error("the broker's profile lists no such order")]
    NotOffered,
    /// A member name the canonical grammar refuses (journal spec §4), which only a change to
    /// this file could introduce.
    #[error("the canonical object could not be built")]
    NonCanonical,
}

impl ProfileError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Unimplemented => "unimplemented",
            Self::VersionZero => "profile_version_zero",
            Self::NoRows => "profile_no_rows",
            Self::DuplicateRow => "profile_duplicate_row",
            Self::EmptyRow => "profile_empty_row",
            Self::DuplicateCell => "profile_duplicate_cell",
            Self::NoTimeInForce => "profile_no_time_in_force",
            Self::ClaimWithoutClientId => "profile_claim_without_client_id",
            Self::NotOffered => "profile_not_offered",
            Self::NonCanonical => "profile_non_canonical",
        }
    }
}
