//! The model provider's key, read once at start (DEC-2020 items 3 and 5, E21-62): the service opens
//! a vault session as [`Identity::ModelGateway`], reads `platform/model-provider-key` once, and
//! keeps it only in memory. A vault outage after the read stops nothing; one during it stops the
//! start. The key is handed to the gateway's transport for the one upstream the routing lock names
//! and to nothing else, and no `Debug`, error or code carries a byte of it.

use std::fmt;

use mandate_gateway::provider::{ProviderKey, SecretError, SecretSource};
use mandate_vault::source::SecretSource as Vault;

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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn code(self) -> &'static str {
        let _ = self;
        todo!()
    }
}

/// The provider's key for one upstream, read once. It answers [`SecretSource::provider_key`] for
/// that upstream alone.
#[derive(Clone)]
#[expect(
    dead_code,
    reason = "the stub reads neither field until the implementation lands (DEC-77)"
)]
pub struct HeldKey {
    upstream: String,
    key: ProviderKey,
}

impl HeldKey {
    /// Reads the key once through `vault`, a session as [`Identity::ModelGateway`], for `upstream`.
    ///
    /// # Errors
    /// [`KeyError`]: another identity's session, no key, a refusal, an outage, or a stored value
    /// that is empty or holds a byte outside printable ASCII, which a header cannot carry intact.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub async fn read<V: Vault>(vault: &V, upstream: &str) -> Result<Self, KeyError> {
        let _ = (vault, upstream);
        todo!()
    }
}

impl SecretSource for HeldKey {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn provider_key(&self, upstream: &str) -> Result<ProviderKey, SecretError> {
        let _ = upstream;
        todo!()
    }
}

impl fmt::Debug for HeldKey {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!()
    }
}
