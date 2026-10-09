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
//! The `Tenant` seal ([identity spec](../../../docs/specs/identity.md) §4.5, DEC-642 item 7,
//! DEC-668): the trait every data API over a workspace takes, its sealing supertrait, and the ID
//! values its methods return.
//!
//! What closes the seal is this crate's `allowed_dependents` list in `xtask/layers.toml`: only
//! `mandate-identity` and `mandate-identity-system` may depend on it, so only they can name
//! [`Sealed`], and only `TenantContext` (and its `Permitted` witness) and `SystemContext` implement
//! [`Tenant`]. Every other crate reaches these items through `mandate-identity`'s re-exports, at
//! the paths they had before (DEC-645 item 3), which name neither [`Sealed`] nor anything that
//! builds a context.

/// A principal's opaque ID, a ULID (identity spec §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalId(pub u128);

/// An organization's opaque ID, a ULID (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrgId(pub u128);

/// A workspace's opaque ID, a ULID (§3.2). No data API takes one bare (ID-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub u128);

/// A session's opaque reference (identity spec §6.3, §12.2): what every committed event names as
/// `session_ref`, never the cookie or the token. For the host CLI, its registration's ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionRef(pub u128);

/// Why a text is not a ULID's: it must be 26 characters of uppercase Crockford base32, the first at
/// most `7`, as the journal validates it (journal spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UlidTextError {
    /// Not 26 characters of uppercase Crockford base32 whose first is at most `7`.
    #[error("not 26 characters of uppercase Crockford base32 whose first is at most 7")]
    Invalid,
}

const ULID_LEN: usize = 26;
const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The 26 uppercase Crockford base32 digits of `value`, most significant first. The first digit is
/// at most `7` because 128 bits leave 3 for it.
fn encode_ulid(mut value: u128) -> Result<String, UlidTextError> {
    let mut reversed = String::with_capacity(ULID_LEN);
    for _ in 0..ULID_LEN {
        let digit = usize::try_from(value % 32).map_err(|_| UlidTextError::Invalid)?;
        let ch = CROCKFORD.chars().nth(digit).ok_or(UlidTextError::Invalid)?;
        reversed.push(ch);
        value /= 32;
    }
    Ok(reversed.chars().rev().collect())
}

/// The value of a canonical ULID text: exactly 26 characters of the uppercase Crockford alphabet,
/// the first at most `7`; anything else, such as lowercase, `I`, `L`, `O`, `U` or a space, is
/// [`UlidTextError::Invalid`].
fn decode_ulid(text: &str) -> Result<u128, UlidTextError> {
    if text.chars().count() != ULID_LEN
        || !text.starts_with(['0', '1', '2', '3', '4', '5', '6', '7'])
    {
        return Err(UlidTextError::Invalid);
    }
    text.chars().try_fold(0u128, |acc, ch| {
        let digit = CROCKFORD
            .chars()
            .position(|c| c == ch)
            .and_then(|d| u128::try_from(d).ok())
            .ok_or(UlidTextError::Invalid)?;
        acc.checked_mul(32)
            .and_then(|shifted| shifted.checked_add(digit))
            .ok_or(UlidTextError::Invalid)
    })
}

impl PrincipalId {
    /// The principal's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The principal a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

impl OrgId {
    /// The organization's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The organization a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

impl WorkspaceId {
    /// The workspace's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The workspace a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

impl SessionRef {
    /// The session reference's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The session reference a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

/// The principal kinds of identity spec §3.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrincipalKind {
    /// A person.
    User,
    /// An owner-connected agent acting for one user (DEC-141).
    Client,
    /// An organization's automation, read and export only in v1.
    ServiceAccount,
    /// A deployed agent's runtime; it holds no column of §4.2.
    Agent,
    /// Executor, scheduler, workspace services; no column of §4.2.
    Process,
    /// The on-host command line of a workspace deployment (§6.4 route 3).
    HostCli,
    /// Platform staff, only inside break-glass (§10.3).
    PlatformOperator,
}

/// What every data API over a workspace takes: a `TenantContext` for a request, or the
/// `SystemContext` of `mandate-identity-system` for the deployment's own background processes
/// (DEC-642 items 3 and 5 to 7). It is sealed by [`Sealed`], which only this crate's allowed
/// dependents can name, so no other type implements it, and none of its implementors takes a bare
/// workspace ID. `mandate-identity` re-exports it as `mandate_identity::Tenant`. It is `Debug`, so
/// a `Permitted` witness can borrow any context as a `&dyn Tenant` (DEC-668 item 4).
pub trait Tenant: Sealed + std::fmt::Debug {
    /// The workspace whose data may be reached.
    fn workspace(&self) -> WorkspaceId;
    /// The workspace's organization.
    fn org(&self) -> OrgId;
    /// Who acts: the authenticated principal, or the process's workload identity.
    fn principal(&self) -> PrincipalId;
    /// The actor's kind.
    fn kind(&self) -> PrincipalKind;
}

/// The supertrait that keeps [`Tenant`] to this crate's allowed dependents. It is never
/// re-exported.
pub trait Sealed {}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
mod tests;
