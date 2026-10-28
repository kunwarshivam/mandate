//! Why a secret source refused. No variant carries a secret, a token, a secret id, or a response
//! body from the vault.

/// A refusal from a [`crate::source::SecretSource`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("this identity's policy does not allow that")]
    Denied,
    #[error("the vault could not be reached")]
    Unavailable,
    #[error("the vault is sealed: an operator must unseal it (DEC-977 item 5)")]
    Sealed,
    #[error("the vault is an OpenBao dev server, which only tests may use")]
    DevServerRefused,
    #[error("the vault answered with something this client does not read")]
    Malformed,
    #[error("the stored entry failed its integrity checks")]
    Integrity,
    #[error("the secret is not there")]
    Missing,
    #[error("the token-exchange grant is unknown, used, expired, or for another connection")]
    GrantInvalid,
    #[error("the vault's address must be https and its CA certificate PEM")]
    Config,
    #[error(
        "a credentials file or directory is missing, not a plain file or directory, or open to others"
    )]
    CredentialFile,
    #[error("the vault has no role for this login yet")]
    RoleMissing,
    #[error("the vault's key-value mount is still finishing the upgrade it starts when enabled")]
    KvUpgrading,
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
}

impl SourceError {
    /// The stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
            Self::Sealed => "sealed",
            Self::DevServerRefused => "dev_server_refused",
            Self::Malformed => "malformed",
            Self::Integrity => "integrity",
            Self::Missing => "missing",
            Self::GrantInvalid => "grant_invalid",
            Self::Config => "config",
            Self::CredentialFile => "credential_file",
            Self::RoleMissing => "role_missing",
            Self::KvUpgrading => "kv_upgrading",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }

    /// Whether the vault may be back later: an outage, not a refusal. E21-9's renewal rule keeps
    /// a lease through these and drops it on any other refusal.
    pub fn is_outage(&self) -> bool {
        matches!(self, Self::Unavailable | Self::Sealed)
    }
}
