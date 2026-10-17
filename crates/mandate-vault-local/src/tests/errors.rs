//! Every error has a stable code, and the keys never print.

use secrecy::SecretBox;

use crate::VaultError;
use crate::startup::Keys;

#[test]
fn every_error_has_its_stable_code() {
    let cases = [
        (VaultError::KeyInEnvironment, "key_in_environment"),
        (VaultError::KeyMissing, "key_missing"),
        (VaultError::KeyMalformed, "key_malformed"),
        (
            VaultError::TokenKeyInApiCredentials,
            "token_key_in_api_credentials",
        ),
        (
            VaultError::DirectoryMismatch { dir: "tokens" },
            "directory_mismatch",
        ),
        (VaultError::Io, "io"),
        (VaultError::InvalidKeyRef, "invalid_key_ref"),
        (VaultError::FileMismatch { dir: "tokens" }, "file_mismatch"),
        (VaultError::EntryMalformed, "entry_malformed"),
        (VaultError::EntryRejected, "entry_rejected"),
        (VaultError::SecretSize, "secret_size"),
        (VaultError::SecretNotPrintable, "secret_not_printable"),
        (VaultError::WrongRole { kind: "code" }, "wrong_role"),
        (VaultError::Random, "random"),
        (
            VaultError::UnknownUser {
                name: "owlhead_exec",
            },
            "unknown_user",
        ),
        (
            VaultError::Unimplemented { story: "E10-13" },
            "unimplemented",
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error:?}");
    }
}

#[test]
fn keys_print_no_key() {
    let keys = Keys {
        pending: SecretBox::new(Box::new([0xAB; 32])),
        token: Some(SecretBox::new(Box::new([0xCD; 32]))),
    };
    assert_eq!(format!("{keys:?}"), "Keys(redacted)");
}
