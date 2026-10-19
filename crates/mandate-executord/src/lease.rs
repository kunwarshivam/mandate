//! The account's credential, held under the vault's lease for as long as the process runs
//! (DEC-2306 item 3; DEC-977 item 7; E21-9).
//!
//! [`Leased`] renews the lease when [`LeaseKeeper`] says one is due and reads the secret back
//! through the reader it is given, so it holds whatever the connection's secret is. Each pass it
//! answers [`Held`]: nothing changed, the secret was rotated and reads as new credentials, or the
//! credential is lost (the lease expired, the vault refused a renewal, or the renewed secret does
//! not read). A lost credential stops the process for its supervisor, which leases again on
//! restart; a vault outage keeps the lease until it expires (`AGENTS.md` rule 3).

use std::fmt;

use mandate_time::UtcNanos;
use mandate_vault::lease::{Lease, LeaseKeeper};
use mandate_vault::source::SecretSource;
use secrecy::{ExposeSecret, SecretSlice};
use tokio::runtime::Runtime;

/// A current-thread runtime to drive one vault call: the process is not async, and the call
/// holds nothing a later pass needs.
fn runtime() -> Result<Runtime, &'static str> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable")
}

/// What one pass found. Its `Debug` names the variant and never the credentials it carries
/// (`AGENTS.md` rule 7).
#[derive(Clone, PartialEq, Eq)]
pub enum Held<C> {
    /// The lease holds the same secret as before.
    Unchanged,
    /// A renewal brought a new secret, which reads as these credentials.
    Rotated(C),
    /// No credential is held any more.
    Lost,
}

impl<C> fmt::Debug for Held<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unchanged => f.write_str("Unchanged"),
            Self::Rotated(_) => f.write_str("Rotated(<redacted>)"),
            Self::Lost => f.write_str("Lost"),
        }
    }
}

/// One connection's credential under the vault's lease, read by `read`.
pub struct Leased<V, R> {
    vault: V,
    keeper: LeaseKeeper,
    read: R,
    /// Set once the credential is lost: only a new lease, on restart, brings one back.
    lost: bool,
}

impl<V: SecretSource, C, E, R: Fn(&[u8]) -> Result<C, E>> Leased<V, R> {
    /// Holds `keeper`'s lease from `vault`, reading each new secret with `read`.
    pub fn new(vault: V, keeper: LeaseKeeper, read: R) -> Self {
        Self {
            vault,
            keeper,
            read,
            lost: false,
        }
    }

    /// One pass at `now`: renews the lease when it is due, then answers what is held.
    ///
    /// # Errors
    /// `runtime_unavailable` when the runtime that drives the vault cannot be built.
    pub fn tick(&mut self, now: UtcNanos) -> Result<Held<C>, &'static str> {
        if self.lost {
            return Ok(Held::Lost);
        }
        let mut rotated = false;
        if self.keeper.renewal_due(now)
            && let Some(held) = self.keeper.lease()
        {
            let outcome = runtime()?.block_on(self.vault.renew(held, now));
            rotated = outcome
                .as_ref()
                .is_ok_and(|renewed| renewed.secret.expose_secret() != held.secret.expose_secret());
            self.keeper.apply(outcome, now);
        }
        let held = match self.keeper.secret(now) {
            None => None,
            Some(secret) if rotated => (self.read)(secret.expose_secret()).ok().map(Held::Rotated),
            Some(_) => Some(Held::Unchanged),
        };
        Ok(held.unwrap_or_else(|| {
            self.lost = true;
            Held::Lost
        }))
    }

    /// Ends the lease on stop, so the vault can take it back before it expires: whatever lease
    /// the keeper still holds, a renewed one whose secret did not read included.
    ///
    /// # Errors
    /// `runtime_unavailable`, or `revoke_failed` when the vault refused or could not be reached.
    pub fn revoke(self) -> Result<(), &'static str> {
        let Some(held) = self.keeper.lease() else {
            return Ok(());
        };
        let lease = Lease {
            connection: held.connection.clone(),
            secret: SecretSlice::from(held.secret.expose_secret().to_vec()),
            expires_at: held.expires_at,
            renew_at: held.renew_at,
        };
        runtime()?
            .block_on(self.vault.revoke(lease))
            .map_err(|_| "revoke_failed")
    }
}
