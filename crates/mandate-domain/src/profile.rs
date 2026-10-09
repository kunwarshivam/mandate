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

use mandate_canon::{Digest, Int, Key, Object, Value, to_canonical};

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
        profile_version: u32,
        rows: Vec<Row>,
        idempotency: Idempotency,
    ) -> Result<Self, ProfileError> {
        if profile_version == 0 {
            return Err(ProfileError::VersionZero);
        }
        if rows.is_empty() {
            return Err(ProfileError::NoRows);
        }
        let claims =
            idempotency.retry != Retry::NotIdempotent || idempotency.query_by_client_order_id;
        if claims && !idempotency.client_order_id {
            return Err(ProfileError::ClaimWithoutClientId);
        }
        let mut sorted = BTreeMap::new();
        for row in &rows {
            let place = (row.asset_class.as_str(), row.session.as_str());
            if sorted.insert(place, row_object(row)?).is_some() {
                return Err(ProfileError::DuplicateRow);
            }
        }
        let cells = rows
            .into_iter()
            .flat_map(|row| {
                row.cells.into_iter().map(move |cell| {
                    let place = (
                        row.asset_class,
                        row.session,
                        cell.order_type,
                        cell.quantity_form,
                    );
                    (place, cell)
                })
            })
            .collect();
        let canonical = object([
            ("idempotency", idempotency_object(idempotency)?),
            ("kind", Value::Str(Self::KIND.to_owned())),
            (
                "profile_version",
                Value::Int(Int::new(u64::from(profile_version)).ok_or(ProfileError::NonCanonical)?),
            ),
            ("rows", Value::Array(sorted.into_values().collect())),
        ])?;
        let content_hash = Digest::of(&to_canonical(&canonical));
        Ok(Self {
            canonical,
            content_hash,
            idempotency,
            cells,
        })
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
        self.cells
            .get(&(asset_class, session, order_type, quantity_form))
            .ok_or(ProfileError::NotOffered)
    }
}

/// One row's object, its cells sorted by order type then quantity form.
fn row_object(row: &Row) -> Result<Value, ProfileError> {
    if row.cells.is_empty() {
        return Err(ProfileError::EmptyRow);
    }
    let mut cells = BTreeMap::new();
    for cell in &row.cells {
        if cell.times_in_force.is_empty() {
            return Err(ProfileError::NoTimeInForce);
        }
        let key = (
            order_type(cell.order_type),
            quantity_form(cell.quantity_form),
        );
        let value = object([
            ("order_type", Value::Str(key.0.to_owned())),
            (
                "protection_forms",
                spellings(cell.protection_forms.iter().map(|&p| protection_form(p))),
            ),
            ("quantity_form", Value::Str(key.1.to_owned())),
            (
                "times_in_force",
                spellings(cell.times_in_force.iter().map(|&t| time_in_force(t))),
            ),
        ])?;
        if cells.insert(key, value).is_some() {
            return Err(ProfileError::DuplicateCell);
        }
    }
    object([
        (
            "asset_class",
            Value::Str(row.asset_class.as_str().to_owned()),
        ),
        ("cells", Value::Array(cells.into_values().collect())),
        ("session", Value::Str(row.session.as_str().to_owned())),
    ])
}

fn idempotency_object(idempotency: Idempotency) -> Result<Value, ProfileError> {
    object([
        ("client_order_id", Value::Bool(idempotency.client_order_id)),
        (
            "query_by_client_order_id",
            Value::Bool(idempotency.query_by_client_order_id),
        ),
        ("retry", Value::Str(retry(idempotency.retry).to_owned())),
    ])
}

fn object<const N: usize>(members: [(&str, Value); N]) -> Result<Value, ProfileError> {
    let mut built = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| ProfileError::NonCanonical)?;
        built.insert(key, value);
    }
    Ok(Value::Object(built))
}

/// A set as an array sorted by spelling, so the hash never depends on a variant's position.
fn spellings<'a>(names: impl Iterator<Item = &'a str>) -> Value {
    let sorted: BTreeSet<&str> = names.collect();
    Value::Array(
        sorted
            .into_iter()
            .map(|n| Value::Str(n.to_owned()))
            .collect(),
    )
}

fn order_type(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Market => "market",
        OrderType::Limit => "limit",
        OrderType::Stop => "stop",
        OrderType::StopLimit => "stop_limit",
    }
}

fn quantity_form(form: QuantityForm) -> &'static str {
    match form {
        QuantityForm::Whole => "whole",
        QuantityForm::Fractional => "fractional",
        QuantityForm::Notional => "notional",
    }
}

fn time_in_force(tif: TimeInForce) -> &'static str {
    match tif {
        TimeInForce::Day => "day",
        TimeInForce::Gtc => "gtc",
        TimeInForce::Ioc => "ioc",
    }
}

fn protection_form(form: ProtectionForm) -> &'static str {
    match form {
        ProtectionForm::Bracket => "bracket",
        ProtectionForm::Oco => "oco",
        ProtectionForm::StopLimit => "stop_limit",
    }
}

fn retry(retry: Retry) -> &'static str {
    match retry {
        Retry::Idempotent => "idempotent",
        Retry::NotIdempotent => "not_idempotent",
        Retry::Unknown => "unknown",
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
