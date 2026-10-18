//! Why the vault refused. No variant carries a key, a code, a token, a secret, or a file name.

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
    #[error("a key_ref is 1 to 100 of A-Z, a-z, 0-9, `.`, `_`, `-` and `/`")]
    InvalidKeyRef,
    #[error("a vault entry in `{dir}` has the wrong owner, group, mode, link count, or type")]
    FileMismatch { dir: &'static str },
    #[error("a vault entry is not in the vault's format")]
    EntryMalformed,
    #[error("a vault entry did not decrypt under its key and binding")]
    EntryRejected,
    #[error("a secret is empty or longer than the vault holds")]
    SecretSize,
    #[error("an imported secret must be printable ASCII with no spaces")]
    SecretNotPrintable,
    #[error("this process's role may not do that to a `{kind}` entry")]
    WrongRole { kind: &'static str },
    #[error("the operating system's random generator failed")]
    Random,
    #[error("the user or group `{name}` is missing from the host's user database")]
    UnknownUser { name: &'static str },
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
            Self::InvalidKeyRef => "invalid_key_ref",
            Self::FileMismatch { .. } => "file_mismatch",
            Self::EntryMalformed => "entry_malformed",
            Self::EntryRejected => "entry_rejected",
            Self::SecretSize => "secret_size",
            Self::SecretNotPrintable => "secret_not_printable",
            Self::WrongRole { .. } => "wrong_role",
            Self::Random => "random",
            Self::UnknownUser { .. } => "unknown_user",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}
