//! The vault's code entry for an Alpaca OAuth connection, format 1
//! ([DEC-2338](../../../docs/project/decisions/DEC-2338.md) item 2): the bytes the API writes at
//! `connections/x<hex>/code` after the callback, and the token-exchange process takes. They are
//! exactly the canonical JSON (journal spec §4)
//!
//! `{"code":"<code>","format":1,"verifier":"<verifier>"}`
//!
//! with a code of 1 to [`MAX_CODE_LEN`] bytes from `!` to `~` other than `"` and `\`, and a
//! verifier of [`MIN_VERIFIER_LEN`] to [`MAX_VERIFIER_LEN`] of RFC 7636's unreserved characters, so
//! neither needs an escape and the entry has one byte form. Decoding compares the bytes with that
//! form around the two values, so nothing else is copied out, and those only into
//! [`secrecy::SecretString`]s (OPS-1).

use secrecy::SecretString;

use crate::ConnectError;
use crate::start::PkceVerifier;
use crate::vault::{AuthorizationCode, WaitingCode};

/// The entry's format; a later shape is a new format, never an optional member.
pub const FORMAT: u64 = 1;

/// The longest code an entry holds, in bytes: the callback's own bound (DEC-2626 item 2).
pub const MAX_CODE_LEN: usize = 512;

/// The shortest verifier RFC 7636 §4.1 allows.
pub const MIN_VERIFIER_LEN: usize = 43;

/// The longest verifier RFC 7636 §4.1 allows.
pub const MAX_VERIFIER_LEN: usize = 128;

/// The entry for `code` and `verifier`.
///
/// # Errors
/// [`ConnectError::CodeEntryInvalid`] for a code or verifier outside the entry's form.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn encode(
    _code: &AuthorizationCode,
    _verifier: &PkceVerifier,
) -> Result<SecretString, ConnectError> {
    todo!()
}

/// The code and verifier of an entry's bytes.
///
/// # Errors
/// [`ConnectError::CodeEntryInvalid`] for any bytes but format 1's exact form; the error names
/// nothing of the bytes.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn decode(_bytes: &[u8]) -> Result<WaitingCode, ConnectError> {
    todo!()
}
