//! The account's credential, held under the vault's lease for as long as the process runs
//! (DEC-2306 item 3; DEC-977 item 7; E21-9).
//!
//! [`Leased`] renews the lease when [`LeaseKeeper`] says one is due and reads the secret back
//! through the reader it is given, so it holds whatever the connection's secret is. Each pass it
//! answers [`Held`]: nothing changed, the secret was rotated and reads as new credentials, or the
//! credential is lost (the lease expired, the vault refused a renewal, or the renewed secret does
//! not read). A lost credential stops the process for its supervisor, which leases again on
//! restart; a vault outage keeps the lease until it expires (`AGENTS.md` rule 3).

use mandate_time::UtcNanos;
use mandate_vault::lease::LeaseKeeper;
use mandate_vault::source::SecretSource;

/// The code every stub of this story's tests PR returns, so every pending test fails at it
/// (DEC-77); the implementation PR replaces the stubs and removes it.
#[allow(
    non_upper_case_globals,
    reason = "the pending gate recognises a stub by the name `Unimplemented` (DEC-137)"
)]
pub const Unimplemented: &str = "unimplemented";

/// What one pass found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held<C> {
    /// The lease holds the same secret as before.
    Unchanged,
    /// A renewal brought a new secret, which reads as these credentials.
    Rotated(C),
    /// No credential is held any more.
    Lost,
}

/// One connection's credential under the vault's lease, read by `read`.
pub struct Leased<V, R> {
    vault: V,
    keeper: LeaseKeeper,
    read: R,
}

impl<V: SecretSource, C, E, R: Fn(&[u8]) -> Result<C, E>> Leased<V, R> {
    /// Holds `keeper`'s lease from `vault`, reading each new secret with `read`.
    pub fn new(vault: V, keeper: LeaseKeeper, read: R) -> Self {
        Self {
            vault,
            keeper,
            read,
        }
    }

    /// One pass at `now`: renews the lease when it is due, then answers what is held.
    ///
    /// # Errors
    /// `runtime_unavailable` when the runtime that drives the vault cannot be built.
    pub fn tick(&mut self, now: UtcNanos) -> Result<Held<C>, &'static str> {
        let _ = (&self.vault, &self.keeper, &self.read, now);
        Err(Unimplemented)
    }

    /// Ends the lease on stop, so the vault can take it back before it expires.
    ///
    /// # Errors
    /// `runtime_unavailable`, or `revoke_failed` when the vault refused or could not be reached.
    pub fn revoke(self) -> Result<(), &'static str> {
        let _ = self;
        Err(Unimplemented)
    }
}
