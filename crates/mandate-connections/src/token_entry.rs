//! The vault's token entry for an Alpaca OAuth connection, format 1
//! ([DEC-2624](../../../docs/project/decisions/DEC-2624.md)): the bytes the token-exchange process
//! writes at `connections/{connection_id}/token` and the connection's executor reads through its
//! lease. They are exactly the canonical JSON (journal spec §4)
//!
//! `{"access_token":"<token>","format":1,"scope":["data","trading"],"token_type":"bearer"}`
//!
//! with a token of 1 to [`MAX_TOKEN_LEN`] bytes from `!` to `~` other than `"` and `\`, so the
//! token needs no escape and the entry has one byte form. Decoding compares the bytes with that
//! form around the token, so nothing but the token is copied out, and only into a
//! [`secrecy::SecretString`] (OPS-1).

use secrecy::SecretString;

use crate::ConnectError;
use crate::grant::GrantedScopes;
use crate::vault::AccessToken;

/// The entry's format; a later shape, an expiry among them, is a new format, never an optional
/// member (DEC-2624 item 3).
pub const FORMAT: u64 = 1;

/// The longest token an entry holds, in bytes.
pub const MAX_TOKEN_LEN: usize = 4096;

/// One decoded entry: the token, and the scopes granted with it.
#[derive(Debug)]
pub struct TokenEntry {
    pub token: AccessToken,
    pub scopes: GrantedScopes,
}

/// The entry for `token` and `scopes`.
///
/// # Errors
/// [`ConnectError::TokenEntryInvalid`] for a token outside the entry's form.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn encode(_token: &AccessToken, _scopes: &GrantedScopes) -> Result<SecretString, ConnectError> {
    todo!()
}

/// The token and scopes of an entry's bytes.
///
/// # Errors
/// [`ConnectError::TokenEntryInvalid`] for any bytes but format 1's exact form; the error names
/// nothing of the bytes.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn decode(_bytes: &[u8]) -> Result<TokenEntry, ConnectError> {
    todo!()
}
