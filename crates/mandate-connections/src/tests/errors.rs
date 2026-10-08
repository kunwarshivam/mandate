//! Every error has a stable code.

use crate::ConnectError;
use crate::record::ConnectionId;

#[test]
fn every_error_has_its_stable_code() {
    let cases = [
        (ConnectError::EnvironmentRefused, "environment_refused"),
        (ConnectError::StateUnknown, "state_unknown"),
        (ConnectError::StateExpired, "state_expired"),
        (ConnectError::StateUserMismatch, "state_user_mismatch"),
        (ConnectError::RequestRefused, "request_refused"),
        (ConnectError::ScopeMismatch, "scope_mismatch"),
        (ConnectError::TokenType, "token_type"),
        (ConnectError::CodeMissing, "code_missing"),
        (ConnectError::VaultUnavailable, "vault_unavailable"),
        (ConnectError::EndpointFailed, "endpoint_failed"),
        (
            ConnectError::AlreadyConnected {
                existing: ConnectionId("conn_a".to_owned()),
            },
            "already_connected",
        ),
        (ConnectError::ReconnectMismatch, "reconnect_mismatch"),
        (ConnectError::FingerprintRotating, "fingerprint_rotating"),
        (ConnectError::InvalidConnectionId, "invalid_connection_id"),
        (ConnectError::InvalidAccountRef, "invalid_account_ref"),
        (ConnectError::InvalidPiiRef, "invalid_pii_ref"),
        (
            ConnectError::InvalidRecord { member: "scopes" },
            "invalid_record",
        ),
        (ConnectError::CheckRefused, "check_refused"),
        (
            ConnectError::Unimplemented { story: "E10-13" },
            "unimplemented",
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error:?}");
    }
}
