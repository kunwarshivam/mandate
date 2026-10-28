//! The news data key, read once at start (DEC-2277 items 3 and 4, DEC-2280 item 6): the reader
//! reads `platform/news-data-key` through its vault session, as the news reader, and keeps the key
//! only as a [`NewsAuth`], whose `Debug` prints nothing of it. A key the vault does not hold is
//! `no_key`, a vault that cannot answer is `vault_unavailable`, and a stored key that is not one
//! line or not a paper key is refused with [`NewsKeyRefusal`]'s own code. None of them carries any
//! byte of the key, and none is retried: the reader stays not ready until it is restarted.

use mandate_sources::fetch::{NewsAuth, NewsKeyRefusal};
use mandate_vault::SourceError;
use mandate_vault::policy::{PlatformSecret, SecretPath};
use mandate_vault::source::SecretSource;
use secrecy::{ExposeSecret, SecretSlice};

/// Why the reader holds no key. Each is a code alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    /// The vault holds no news data key: the founder has not imported one (DEC-2277 item 5).
    NoKey,
    /// The vault could not answer, or refused the read: an outage, not a refusal of the key.
    VaultUnavailable,
    /// The stored key is not one line `<key id>:<secret>`, or not a paper key.
    Refused(NewsKeyRefusal),
}

impl KeyRefusal {
    /// The code the reader writes on its standard error (DEC-2280 item 6).
    pub fn code(self) -> &'static str {
        match self {
            Self::NoKey => "no_key",
            Self::VaultUnavailable => "vault_unavailable",
            Self::Refused(refusal) => refusal.code(),
        }
    }
}

/// The key from the vault's answer to the one read: `Ok(None)` or `Missing` is [`KeyRefusal::NoKey`],
/// any other error [`KeyRefusal::VaultUnavailable`], and a stored value [`NewsAuth::from_stored`]
/// refuses is [`KeyRefusal::Refused`].
///
/// # Errors
/// [`KeyRefusal`], as above.
pub fn stored_key(
    answer: Result<Option<SecretSlice<u8>>, SourceError>,
) -> Result<NewsAuth, KeyRefusal> {
    let stored = match answer {
        Ok(Some(stored)) => stored,
        Ok(None) | Err(SourceError::Missing) => return Err(KeyRefusal::NoKey),
        Err(_) => return Err(KeyRefusal::VaultUnavailable),
    };
    NewsAuth::from_stored(stored.expose_secret()).map_err(KeyRefusal::Refused)
}

/// Reads the key once through `vault`, a session as the vault's news reader.
///
/// # Errors
/// [`stored_key`]'s.
pub async fn read_key<V: SecretSource>(vault: &V) -> Result<NewsAuth, KeyRefusal> {
    stored_key(
        vault
            .read(&SecretPath::Platform(PlatformSecret::NewsDataKey))
            .await,
    )
}
