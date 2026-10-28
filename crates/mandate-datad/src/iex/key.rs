//! The platform data key, as the data process's IEX source reads it (DEC-2283 item 2, DEC-2277):
//! logged in to the vault by AppRole as [`Identity::BarsReader`] with the credentials and CA
//! mounted in one directory, read once from `platform/news-data-key`, the session then ended so
//! its token outlives the read by no more than one request (E2-22), and kept only as the bars
//! transport's [`Credentials`], whose `Debug` shows nothing. Every refusal is a code that names no
//! part of the key.

use std::path::Path;

use mandate_marketdata::http::Credentials;
use mandate_time::UtcNanos;
use mandate_vault::mounted::{read_mounted, read_mounted_credentials};
use mandate_vault::openbao::{OpenBaoConfig, OpenBaoSource};
use mandate_vault::policy::{Identity, Mounts, PlatformSecret, SecretPath};
use mandate_vault::source::SecretSource;
use secrecy::ExposeSecret;

/// The mounted file holding the vault's CA.
pub const VAULT_CA: &str = "ca.crt";

/// The vault at `address`, trusting the CA mounted in `dir`.
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

/// The platform data key, read once as the bars reader at `now`.
///
/// # Errors
/// `vault_credentials_unreadable`, `vault_login_failed`, `data_key_unreadable` or
/// `data_key_missing` when the vault does not hand it over, `vault_session_not_ended` when the
/// vault does not revoke the session after, and the key's own refusal
/// (`malformed`, `not_paper`) when it is not one paper key's line.
pub async fn platform_key(
    config: &OpenBaoConfig,
    dir: &Path,
    now: UtcNanos,
) -> Result<Credentials, &'static str> {
    let (role_id, secret_id) = read_mounted_credentials(dir, &Identity::BarsReader)
        .map_err(|_| "vault_credentials_unreadable")?;
    let session = OpenBaoSource::login(config, Identity::BarsReader, &role_id, &secret_id, now)
        .await
        .map_err(|_| "vault_login_failed")?;
    let held = session
        .read(&SecretPath::Platform(PlatformSecret::NewsDataKey))
        .await
        .map_err(|_| "data_key_unreadable");
    let ended = session.end_session().await;
    let held = held?.ok_or("data_key_missing")?;
    ended.map_err(|_| "vault_session_not_ended")?;
    Credentials::from_stored(held.expose_secret()).map_err(|refusal| refusal.code())
}
