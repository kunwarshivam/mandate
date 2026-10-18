//! What one vault entry is: its `key_ref`, its kind, and what it is bound to (DEC-692 item 4).
//! The associated data of its encryption binds all three, so a file renamed onto another
//! connection's name, or moved between kinds, fails to decrypt.

use mandate_connections::record::{ConnectionId, Environment};

use crate::VaultError;

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
        let allowed = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/');
        if (1..=MAX_KEY_REF_LEN).contains(&text.len()) && text.bytes().all(allowed) {
            Ok(Self(text.to_owned()))
        } else {
            Err(VaultError::InvalidKeyRef)
        }
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
        let mut out = ASSOCIATED_DATA_TAG.to_vec();
        push_field(&mut out, self.kind.as_str());
        push_field(&mut out, self.key_ref.as_str());
        match &self.binding {
            Binding::Connection {
                connection_id,
                environment,
            } => {
                push_field(&mut out, "connection");
                push_field(&mut out, connection_id.as_str());
                push_field(&mut out, environment.as_str());
            }
            Binding::Platform => push_field(&mut out, "platform"),
        }
        out
    }

    /// Its file name: the `key_ref` in lowercase hex, then a dot and the kind, so two kinds under
    /// one `key_ref` never share a file.
    pub fn file_name(&self) -> String {
        let mut name = String::with_capacity(self.key_ref.0.len().saturating_mul(2));
        for byte in self.key_ref.0.bytes() {
            name.push(hex_digit(byte >> 4));
            name.push(hex_digit(byte & 0x0F));
        }
        name.push('.');
        name.push_str(self.kind.as_str());
        name
    }
}

/// The version tag the associated data starts with.
const ASSOCIATED_DATA_TAG: &[u8] = b"mandate-vault-local/v1\0";

/// Appends one field as a two-byte big-endian length and its bytes. Every field is a kind, a
/// binding tag, an environment, a [`KeyRef`], or a connection id, so none is longer than
/// [`MAX_KEY_REF_LEN`] and the length always fits.
fn push_field(out: &mut Vec<u8>, field: &str) {
    let len = u16::try_from(field.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(field.as_bytes());
}

/// One lowercase hex digit of a nibble.
fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}
