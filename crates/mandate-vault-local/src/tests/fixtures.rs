//! Entries the tests share, and a canary secret that must never be seen outside the vault.

use mandate_connections::record::{ConnectionId, Environment};
use secrecy::SecretSlice;

use crate::entry::{ALPACA_CLIENT_SECRET_KEY_REF, Entry, KeyRef};

/// A secret no output may contain: if it shows up in `Debug`, an error, a file, or the CLI's
/// output, that output leaked a credential.
pub const CANARY: &str = "canary-7f3c9e1a-vault-secret-do-not-print";

pub fn secret(text: &str) -> SecretSlice<u8> {
    SecretSlice::from(text.as_bytes().to_vec())
}

pub fn key_ref(text: &str) -> KeyRef {
    KeyRef::new(text).unwrap()
}

pub fn conn(text: &str) -> ConnectionId {
    ConnectionId::new(text).unwrap()
}

pub fn code(key: &str, connection: &str) -> Entry {
    Entry::code(key_ref(key), conn(connection), Environment::Paper)
}

pub fn token(key: &str, connection: &str) -> Entry {
    Entry::token(key_ref(key), conn(connection), Environment::Paper)
}

pub fn client_secret() -> Entry {
    Entry::client_secret(key_ref(ALPACA_CLIENT_SECRET_KEY_REF))
}

/// Asserts `text` holds no part of the canary long enough to identify it.
pub fn assert_no_canary(text: &str, what: &str) {
    assert!(!text.contains(CANARY), "{what} printed the canary: {text}");
    assert!(
        !text.contains(&CANARY[..16]),
        "{what} printed part of the canary: {text}"
    );
}

/// Asserts `bytes` holds no copy of the canary.
pub fn assert_no_canary_bytes(bytes: &[u8], what: &str) {
    assert!(
        !bytes.windows(16).any(|w| w == &CANARY.as_bytes()[..16]),
        "{what} holds the canary in the clear"
    );
}
