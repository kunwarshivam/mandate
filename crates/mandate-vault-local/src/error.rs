//! Why the vault refused. No variant carries a key, a code, a token, or a secret.

/// A refusal from the local vault.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    #[error("a vault key was supplied through the environment")]
    KeyInEnvironment,
    #[error("a required vault key credential is missing")]
    KeyMissing,
    #[error("a vault key credential is not exactly 32 bytes")]
    KeyMalformed,
    #[error("the API process was given the token key")]
    TokenKeyInApiCredentials,
    #[error("the vault directory `{dir}` has the wrong owner, group, mode, or type")]
    DirectoryMismatch { dir: &'static str },
    #[error("the vault could not read its files")]
    Io,
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
}

impl VaultError {
    /// The stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::KeyInEnvironment => "key_in_environment",
            Self::KeyMissing => "key_missing",
            Self::KeyMalformed => "key_malformed",
            Self::TokenKeyInApiCredentials => "token_key_in_api_credentials",
            Self::DirectoryMismatch { .. } => "directory_mismatch",
            Self::Io => "io",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}
