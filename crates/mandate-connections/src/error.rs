//! Why a connect step refused. No variant carries a credential, a code, or broker text (CN-1).

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
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}
