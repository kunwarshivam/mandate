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
//! The seal of `mandate-identity`'s session, membership, and membership-lookup types (identity
//! spec §4.5, DEC-642 item 7).
//!
//! `mandate_identity::Session::new` and `mandate_identity::Membership::new` take a [`Seal`], and
//! `mandate_identity::MembershipLookup` has [`LookupSeal`] as its supertrait. What closes them is
//! this crate's `allowed_dependents` list in `xtask/layers.toml`: only `mandate-identity`,
//! `mandate-identity-testkit`, and (when they exist) `mandate-authn` and the workspace store may
//! depend on it, so no other crate can name either item, build a session or a membership, or
//! hand `authorize` a lookup of its own.

/// The token a sealed constructor takes. Holding one is the proof that the caller is an allowed
/// dependent of this crate.
#[derive(Debug)]
pub struct Seal(());

impl Seal {
    /// A token, for an allowed dependent.
    pub const fn grant() -> Self {
        Self(())
    }
}

/// The supertrait that keeps `mandate_identity::MembershipLookup` to the allowed dependents.
pub trait LookupSeal {}

#[cfg(test)]
mod tests {
    use super::Seal;

    /// A grant is the one token there is: it carries nothing a holder could vary.
    #[test]
    fn a_grant_is_a_bare_token() {
        assert_eq!(format!("{:?}", Seal::grant()), "Seal(())");
    }
}
