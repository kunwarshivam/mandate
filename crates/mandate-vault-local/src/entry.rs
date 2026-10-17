//! What one vault entry is: its `key_ref`, its kind, and what it is bound to (DEC-692 item 4).
//! The associated data of its encryption binds all three, so a file renamed onto another
//! connection's name, or moved between kinds, fails to decrypt.

use mandate_connections::record::{ConnectionId, Environment};

use crate::VaultError;

/// The story every stub in this crate names until it lands.
pub(crate) const STORY: &str = "E10-13 V1";

/// The `key_ref` of the platform's Alpaca OAuth client secret (DEC-692 item 6).
pub const ALPACA_CLIENT_SECRET_KEY_REF: &str = "platform/alpaca-oauth-client-secret";

/// The longest `key_ref`, in bytes. Its file name is twice that in hex plus a suffix, well under
/// the 255 bytes a file name may have.
pub const MAX_KEY_REF_LEN: usize = 100;

/// What an entry holds. Codes live in `pending/`; tokens and the client secret in `tokens/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An OAuth authorization code with its PKCE verifier; the API writes it.
    Code,
    /// An access token with its granted scopes; the executor writes it.
    Token,
    /// The platform's OAuth client secret; `mandate vault import` writes it as the executor.
    ClientSecret,
}

impl Kind {
    /// As the associated data and a refusal name it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Token => "token",
            Self::ClientSecret => "client_secret",
        }
    }

    /// The vault subdirectory that holds this kind.
    pub fn dir(self) -> &'static str {
        match self {
            Self::Code => "pending",
            Self::Token | Self::ClientSecret => "tokens",
        }
    }
}

/// A vault path: 1 to [`MAX_KEY_REF_LEN`] bytes of `A-Z`, `a-z`, `0-9`, `.`, `_`, `-` and `/`.
/// Not a secret, but never sent to a client (connections spec §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRef(String);

impl KeyRef {
    pub fn new(text: &str) -> Result<Self, VaultError> {
        let _ = text;
        Err(VaultError::Unimplemented { story: STORY })
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What an entry belongs to: one connection in one environment, or the platform itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    Connection {
        connection_id: ConnectionId,
        environment: Environment,
    },
    Platform,
}

/// One entry: the only way to name a file in the vault. Its constructors pair each kind with the
/// binding it may have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    key_ref: KeyRef,
    kind: Kind,
    binding: Binding,
}

impl Entry {
    /// A connection's waiting authorization code.
    pub fn code(key_ref: KeyRef, connection_id: ConnectionId, environment: Environment) -> Self {
        Self {
            key_ref,
            kind: Kind::Code,
            binding: Binding::Connection {
                connection_id,
                environment,
            },
        }
    }

    /// A connection's access token.
    pub fn token(key_ref: KeyRef, connection_id: ConnectionId, environment: Environment) -> Self {
        Self {
            key_ref,
            kind: Kind::Token,
            binding: Binding::Connection {
                connection_id,
                environment,
            },
        }
    }

    /// The platform's client secret under `key_ref`.
    pub fn client_secret(key_ref: KeyRef) -> Self {
        Self {
            key_ref,
            kind: Kind::ClientSecret,
            binding: Binding::Platform,
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn key_ref(&self) -> &KeyRef {
        &self.key_ref
    }

    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    /// The associated data its encryption binds: a version tag, then the kind, the `key_ref`, and
    /// the binding, each length-prefixed so no two entries share it.
    pub fn associated_data(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Its file name: the `key_ref` in lowercase hex, then a dot and the kind, so two kinds under
    /// one `key_ref` never share a file.
    pub fn file_name(&self) -> String {
        String::new()
    }
}
