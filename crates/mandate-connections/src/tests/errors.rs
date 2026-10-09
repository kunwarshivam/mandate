//! Every error has a stable code, and no secret wrapper prints its secret.

use secrecy::SecretString;

use super::support::{CANARIES, CODE, SECRET, TOKEN};
use crate::ConnectError;
use crate::exchange::{ClientSecret, TokenResponse};
use crate::vault::{AccessToken, AuthorizationCode};

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

#[test]
fn secret_wrappers_print_no_secret() {
    let printed = [
        (
            format!("{:?}", AuthorizationCode(SecretString::from(CODE))),
            "AuthorizationCode(redacted)",
        ),
        (
            format!("{:?}", AccessToken(SecretString::from(TOKEN))),
            "AccessToken(redacted)",
        ),
        (
            format!("{:?}", ClientSecret(SecretString::from(SECRET))),
            "ClientSecret(redacted)",
        ),
    ];
    for (text, expected) in printed {
        assert_eq!(text, expected);
    }
    let response = format!(
        "{:?}",
        TokenResponse {
            access_token: AccessToken(SecretString::from(TOKEN)),
            token_type: "bearer".to_owned(),
            scope: "trading data".to_owned(),
        }
    );
    for canary in CANARIES {
        assert!(!response.contains(canary), "{response}");
    }
}
