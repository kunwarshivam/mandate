//! The model provider's key, read once at start (DEC-2020 items 3 and 5, E21-62): the service opens
//! a vault session as [`Identity::ModelGateway`], reads `platform/model-provider-key` once, and
//! keeps it only in memory. A vault outage after the read stops nothing; one during it stops the
//! start. The key is handed to the gateway's transport for the one upstream the routing lock names
//! and to nothing else, and no `Debug`, error or code carries a byte of it.

use std::collections::BTreeSet;
use std::fmt;

use mandate_gateway::provider::{ProviderKey, SecretError, SecretSource};
use mandate_vault::SourceError;
use mandate_vault::policy::{Identity, PlatformSecret, SecretPath};
use mandate_vault::source::SecretSource as Vault;
use secrecy::ExposeSecret;

/// Why the key could not be held. Each names only its code, never the key or the vault's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("the vault session is not the model gateway's")]
    Identity,
    #[error("the vault holds no model provider key")]
    Missing,
    #[error("the vault's policy refuses the model gateway this read")]
    Denied,
    #[error("the vault cannot be read now")]
    Unavailable,
    #[error("the stored value is not a key a request header can carry")]
    Malformed,
}

impl KeyError {
    /// The code the process exits with.
    pub fn code(self) -> &'static str {
        match self {
            Self::Identity => "model_key_identity",
            Self::Missing => "model_key_missing",
            Self::Denied => "model_key_denied",
            Self::Unavailable => "model_key_unavailable",
            Self::Malformed => "model_key_malformed",
        }
    }
}

/// The provider's key, read once, for the upstreams the start names. It answers
/// [`SecretSource::provider_key`] for those upstreams alone.
#[derive(Clone)]
pub struct HeldKey {
    upstreams: BTreeSet<String>,
    key: ProviderKey,
}

impl HeldKey {
    /// Reads the key once through `vault`, a session as [`Identity::ModelGateway`], for `upstream`.
    ///
    /// # Errors
    /// [`KeyError`]: another identity's session, no key, a refusal, an outage, or a stored value
    /// that is empty or holds a byte outside printable ASCII, which a header cannot carry intact.
    pub async fn read<V: Vault>(vault: &V, upstream: &str) -> Result<Self, KeyError> {
        if *vault.identity() != Identity::ModelGateway {
            return Err(KeyError::Identity);
        }
        let stored = vault
            .read(&SecretPath::Platform(PlatformSecret::ModelProviderKey))
            .await
            .map_err(|error| match error {
                SourceError::Missing => KeyError::Missing,
                SourceError::Denied => KeyError::Denied,
                _ => KeyError::Unavailable,
            })?
            .ok_or(KeyError::Missing)?;
        let bytes = stored.expose_secret();
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_graphic) {
            return Err(KeyError::Malformed);
        }
        let key = String::from_utf8(bytes.to_vec()).map_err(|_| KeyError::Malformed)?;
        Ok(Self {
            upstreams: BTreeSet::from([upstream.to_owned()]),
            key: ProviderKey::new(key),
        })
    }

    /// The same key, served for `upstream` too, with no new read: a live registry's entries may
    /// route through more than one upstream (DEC-2216).
    #[must_use]
    pub fn with_upstream(mut self, upstream: &str) -> Self {
        self.upstreams.insert(upstream.to_owned());
        self
    }
}

impl SecretSource for HeldKey {
    fn provider_key(&self, upstream: &str) -> Result<ProviderKey, SecretError> {
        if self.upstreams.contains(upstream) {
            Ok(self.key.clone())
        } else {
            Err(SecretError::Missing)
        }
    }
}

impl fmt::Debug for HeldKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeldKey")
            .field("upstreams", &self.upstreams)
            .finish_non_exhaustive()
    }
}
