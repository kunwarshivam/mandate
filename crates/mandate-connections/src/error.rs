//! Why a connect step refused. No variant carries a credential, a code, or broker text (CN-1),
//! and none carries an account fingerprint: errors reach API responses (API-11).

use crate::record::{ConnectionId, ConnectionState};

/// A refusal from the connect core.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectError {
    #[error("only a paper connection may use Alpaca OAuth (DEC-821)")]
    EnvironmentRefused,
    #[error("the state is unknown")]
    StateUnknown,
    #[error("the state has expired")]
    StateExpired,
    #[error("the state was issued to another user")]
    StateUserMismatch,
    #[error("the state is already issued")]
    StateReused,
    #[error("the request may not leave the process")]
    RequestRefused,
    #[error("the granted scopes differ from the requested ones")]
    ScopeMismatch,
    #[error("the token response is not a bearer token")]
    TokenType,
    #[error("no authorization code is waiting in the vault")]
    CodeMissing,
    #[error("the vault is unavailable")]
    VaultUnavailable,
    #[error("the token endpoint failed")]
    EndpointFailed,
    #[error("the account is already connected as {}", existing.as_str())]
    AlreadyConnected { existing: ConnectionId },
    #[error("a reconnect must keep its connection's broker and environment")]
    ReconnectMismatch,
    #[error("the account fingerprint key is being rotated; connects resume when it finishes")]
    FingerprintRotating,
    #[error("a connection id is 1 to 64 of A-Z, a-z, 0-9, _ and -")]
    InvalidConnectionId,
    #[error("an account reference is a ULID")]
    InvalidAccountRef,
    #[error("a personal-data reference is 1 to 64 of A-Z, a-z, 0-9, _ and -")]
    InvalidPiiRef,
    #[error("the connection record's {member} breaks connections spec §3 to §6")]
    InvalidRecord { member: &'static str },
    #[error("a permission check refused the credential")]
    CheckRefused,
    #[error("no connection record has this id")]
    UnknownConnection,
    #[error("a connection does not move from {from:?} to {to:?} (connections spec §9.1)")]
    InvalidTransition {
        from: ConnectionState,
        to: ConnectionState,
    },
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
}

impl ConnectError {
    /// The stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::EnvironmentRefused => "environment_refused",
            Self::StateUnknown => "state_unknown",
            Self::StateExpired => "state_expired",
            Self::StateUserMismatch => "state_user_mismatch",
            Self::StateReused => "state_reused",
            Self::RequestRefused => "request_refused",
            Self::ScopeMismatch => "scope_mismatch",
            Self::TokenType => "token_type",
            Self::CodeMissing => "code_missing",
            Self::VaultUnavailable => "vault_unavailable",
            Self::EndpointFailed => "endpoint_failed",
            Self::AlreadyConnected { .. } => "already_connected",
            Self::ReconnectMismatch => "reconnect_mismatch",
            Self::FingerprintRotating => "fingerprint_rotating",
            Self::InvalidConnectionId => "invalid_connection_id",
            Self::InvalidAccountRef => "invalid_account_ref",
            Self::InvalidPiiRef => "invalid_pii_ref",
            Self::InvalidRecord { .. } => "invalid_record",
            Self::CheckRefused => "check_refused",
            Self::UnknownConnection => "unknown_connection",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}
