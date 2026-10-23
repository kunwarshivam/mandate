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
//! The role provisioner's process (E21-33, DEC-2350): `mandate-provisioner`'s `Provisioner` over
//! the workspace control stream and the vault, with `mandate-process`'s lifecycle and probes. The
//! provisioner decides; this crate only wires its ports, keeps its vault session logged in, and
//! folds each pass's outcome into the lifecycle. The control stream and the alerts are the
//! deployment manager's own (`mandate-deployd`), since both processes write that one stream the
//! same way.

use std::path::Path;

use mandate_process::Input;
use mandate_provisioner::driver::Halt;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::LEASE_SECS;
use mandate_vault::mounted::{read_mounted, read_mounted_credentials};
use mandate_vault::openbao::{OpenBaoConfig, OpenBaoSource};
use mandate_vault::policy::{Identity, Mounts};
use secrecy::ExposeSecret;

/// The fixture vault `run --fixture` uses; a build without debug assertions has none.
#[cfg(debug_assertions)]
pub mod fixture;

/// The vault's CA certificate in the mounted vault directory.
pub const VAULT_CA: &str = "ca.crt";

/// How long a vault session is used before the process logs in again: an hour, well inside the
/// provisioner's day-long AppRole token, so a vault that refuses logins for most of a day still
/// leaves the process a live token.
pub const RELOGIN_SECS: i64 = 3_600;

/// How long after a refused login the process tries again, so a vault that is down is not asked
/// once a pass.
pub const RELOGIN_RETRY_SECS: i64 = 60;

/// How long before its token's end a session stops being used: longer than one pass's vault
/// deadline, so no call is still in flight when the token lapses.
pub const EXPIRY_MARGIN_SECS: i64 = 300;

const _RENEWED_BEFORE_THE_END: () = assert!(
    RELOGIN_SECS < LEASE_SECS - EXPIRY_MARGIN_SECS,
    "a session is renewed well before it stops being usable"
);

/// What one pass reports to the lifecycle. A pass that could not read or write the journal, or
/// that found the stream not yet opened, is a heartbeat, and the next pass starts again from the
/// journal; a record the journal refuses is a defect, so the process stops rather than retry it
/// blind.
pub fn input(pass: &Result<(), Halt>) -> Input {
    match pass {
        Ok(()) => Input::Heartbeat,
        Err(Halt::Refused { .. }) => Input::Failed,
        Err(Halt::Journal | Halt::NotOpened) => Input::Heartbeat,
    }
}

/// When the vault session in use was taken and when it is next renewed. Its token is the
/// provisioner's AppRole token, which `mandate_vault::openbao::provision` gives a fixed
/// `LEASE_SECS` with no renewal past it; the vault counts that from its own receipt of the
/// login, which is after the `now` it is reckoned from here, so the end reckoned here is never
/// later than the token's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Renewal {
    usable_until: i64,
    next_login: i64,
}

impl Renewal {
    /// A session taken by a login at `now`.
    pub fn new(now: UtcNanos) -> Self {
        Self {
            usable_until: now
                .secs()
                .saturating_add(LEASE_SECS)
                .saturating_sub(EXPIRY_MARGIN_SECS),
            next_login: now.secs().saturating_add(RELOGIN_SECS),
        }
    }

    /// Whether a new login is due at `now`.
    pub fn due(&self, now: UtcNanos) -> bool {
        now.secs() >= self.next_login
    }

    /// A refused login at `now`: the session in use stays, and the next login is a minute later.
    pub fn refused(&mut self, now: UtcNanos) {
        self.next_login = now.secs().saturating_add(RELOGIN_RETRY_SECS);
    }

    /// Whether the session in use may still be used at `now`. Once it may not, the process makes
    /// and removes no role: it never calls the vault with a token at or near its end.
    pub fn usable(&self, now: UtcNanos) -> bool {
        now.secs() < self.usable_until
    }
}

/// Why the start is refused before anything is read. None carries a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRefused {
    /// `--fixture` was asked of a build without debug assertions, which holds no fixture vault.
    FixtureNotBuilt,
}

impl StartRefused {
    pub fn code(self) -> &'static str {
        match self {
            Self::FixtureNotBuilt => "fixture_not_built",
        }
    }
}

/// Whether the start may run with the fixture when `fixture` was asked, in a build with debug
/// assertions or not (`debug_build`): a release build holds no fixture, so asking for it is
/// refused, the stub issuer's release pin (DEC-2100 item 2).
///
/// # Errors
/// [`StartRefused::FixtureNotBuilt`].
pub fn fixture_allowed(fixture: bool, debug_build: bool) -> Result<bool, StartRefused> {
    match (fixture, debug_build) {
        (true, false) => Err(StartRefused::FixtureNotBuilt),
        (fixture, _) => Ok(fixture),
    }
}

/// The vault at `address`, trusting only the CA mounted in `dir`.
///
/// # Errors
/// `vault_ca_unreadable` when the CA cannot be read.
pub fn vault_config(address: &str, dir: &Path) -> Result<OpenBaoConfig, &'static str> {
    let ca = read_mounted(dir, VAULT_CA).map_err(|_| "vault_ca_unreadable")?;
    Ok(OpenBaoConfig {
        address: address.to_owned(),
        ca_pem: ca.expose_secret().as_bytes().to_vec(),
        mounts: Mounts::default(),
    })
}

/// A vault session as [`Identity::Provisioner`], logged in by AppRole with the credentials
/// mounted in `dir`. They are read again at every login, so a rotated secret id is picked up.
///
/// # Errors
/// `vault_credentials_unreadable` when they cannot be read, and `vault_login_failed` when the
/// vault cannot be reached or refuses them. Neither carries a credential.
pub async fn vault_login(
    config: &OpenBaoConfig,
    dir: &Path,
    now: UtcNanos,
) -> Result<OpenBaoSource, &'static str> {
    let (role_id, secret_id) = read_mounted_credentials(dir, &Identity::Provisioner)
        .map_err(|_| "vault_credentials_unreadable")?;
    OpenBaoSource::login(config, Identity::Provisioner, &role_id, &secret_id, now)
        .await
        .map_err(|_: SourceError| "vault_login_failed")
}
