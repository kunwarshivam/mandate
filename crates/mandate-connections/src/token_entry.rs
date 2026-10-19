//! The vault's token entry for an Alpaca OAuth connection, format 1
//! ([DEC-2624](../../../docs/project/decisions/DEC-2624.md)): the bytes the token-exchange process
//! writes at `connections/x<hex>/token`, the connection id's hex after an `x`
//! ([DEC-2060](../../../docs/project/decisions/DEC-2060.md) item 3), and the connection's executor
//! reads through its lease. They are exactly the canonical JSON (journal spec §4)
//!
//! `{"access_token":"<token>","format":1,"scope":["data","trading"],"token_type":"bearer"}`
//!
//! with a token of 1 to [`MAX_TOKEN_LEN`] bytes from `!` to `~` other than `"` and `\`, so the
//! token needs no escape and the entry has one byte form. Decoding compares the bytes with that
//! form around the token, so nothing but the token is copied out, and only into a
//! [`secrecy::SecretString`] (OPS-1).

use secrecy::{ExposeSecret, SecretString};

use crate::ConnectError;
use crate::grant::{GrantedScopes, REQUESTED_SCOPES_SORTED};
use crate::vault::AccessToken;

/// The entry's format; a later shape, an expiry among them, is a new format, never an optional
/// member (DEC-2624 item 3).
pub const FORMAT: u64 = 1;

/// The longest token an entry holds, in bytes.
pub const MAX_TOKEN_LEN: usize = 4096;

/// The entry's bytes before the token.
const BEFORE: &str = r#"{"access_token":""#;

/// The entry's bytes after the token: format 1, the requested scopes sorted, and the token type.
const AFTER: &str = r#"","format":1,"scope":["data","trading"],"token_type":"bearer"}"#;

/// Whether `token` is 1 to [`MAX_TOKEN_LEN`] bytes from `!` to `~`, none of them `"` or `\`.
fn in_form(token: &str) -> bool {
    (1..=MAX_TOKEN_LEN).contains(&token.len())
        && token
            .bytes()
            .all(|b| (b'!'..=b'~').contains(&b) && b != b'"' && b != b'\\')
}

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
pub fn encode(token: &AccessToken, scopes: &GrantedScopes) -> Result<SecretString, ConnectError> {
    let token = token.0.expose_secret();
    if !in_form(token) || !scopes.iter().eq(REQUESTED_SCOPES_SORTED) {
        return Err(ConnectError::TokenEntryInvalid);
    }
    Ok(SecretString::from(format!("{BEFORE}{token}{AFTER}")))
}

/// The token and scopes of an entry's bytes.
///
/// # Errors
/// [`ConnectError::TokenEntryInvalid`] for any bytes but format 1's exact form; the error names
/// nothing of the bytes.
pub fn decode(bytes: &[u8]) -> Result<TokenEntry, ConnectError> {
    let token = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_prefix(BEFORE))
        .and_then(|rest| rest.strip_suffix(AFTER))
        .filter(|token| in_form(token))
        .ok_or(ConnectError::TokenEntryInvalid)?;
    Ok(TokenEntry {
        token: AccessToken(SecretString::from(token.to_owned())),
        scopes: GrantedScopes(REQUESTED_SCOPES_SORTED.map(str::to_owned).into()),
    })
}
