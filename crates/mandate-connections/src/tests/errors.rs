//! Every error has a stable code.

use crate::ConnectError;

#[test]
fn every_error_has_its_stable_code() {
    let cases = [
        (ConnectError::EnvironmentRefused, "environment_refused"),
        (ConnectError::StateUnknown, "state_unknown"),
        (ConnectError::StateExpired, "state_expired"),
        (ConnectError::StateUserMismatch, "state_user_mismatch"),
        (ConnectError::StateReused, "state_reused"),
        (ConnectError::RequestRefused, "request_refused"),
        (ConnectError::ScopeMismatch, "scope_mismatch"),
        (ConnectError::TokenType, "token_type"),
        (ConnectError::CodeMissing, "code_missing"),
        (ConnectError::VaultUnavailable, "vault_unavailable"),
        (ConnectError::EndpointFailed, "endpoint_failed"),
        (
            ConnectError::Unimplemented { story: "E10-13" },
            "unimplemented",
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error:?}");
    }
}
