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

use secrecy::{ExposeSecret, SecretString};

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

/// The entry's bytes before the code.
const BEFORE: &str = r#"{"code":""#;

/// The entry's bytes between the code and the verifier.
const BETWEEN: &str = r#"","format":1,"verifier":""#;

/// The entry's bytes after the verifier.
const AFTER: &str = r#""}"#;

/// Whether `code` is 1 to [`MAX_CODE_LEN`] bytes from `!` to `~`, none of them `"` or `\`.
fn code_in_form(code: &str) -> bool {
    (1..=MAX_CODE_LEN).contains(&code.len())
        && code
            .bytes()
            .all(|b| (b'!'..=b'~').contains(&b) && b != b'"' && b != b'\\')
}

/// Whether `verifier` is [`MIN_VERIFIER_LEN`] to [`MAX_VERIFIER_LEN`] unreserved characters.
fn verifier_in_form(verifier: &str) -> bool {
    (MIN_VERIFIER_LEN..=MAX_VERIFIER_LEN).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// The entry for `code` and `verifier`.
///
/// # Errors
/// [`ConnectError::CodeEntryInvalid`] for a code or verifier outside the entry's form.
pub fn encode(
    code: &AuthorizationCode,
    verifier: &PkceVerifier,
) -> Result<SecretString, ConnectError> {
    let code = code.0.expose_secret();
    let verifier = verifier.0.expose_secret();
    if !code_in_form(code) || !verifier_in_form(verifier) {
        return Err(ConnectError::CodeEntryInvalid);
    }
    Ok(SecretString::from(format!(
        "{BEFORE}{code}{BETWEEN}{verifier}{AFTER}"
    )))
}

/// The code and verifier of an entry's bytes.
///
/// # Errors
/// [`ConnectError::CodeEntryInvalid`] for any bytes but format 1's exact form; the error names
/// nothing of the bytes.
pub fn decode(bytes: &[u8]) -> Result<WaitingCode, ConnectError> {
    let (code, verifier) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_prefix(BEFORE))
        .and_then(|rest| rest.strip_suffix(AFTER))
        .and_then(|rest| rest.split_once(BETWEEN))
        .filter(|(code, verifier)| code_in_form(code) && verifier_in_form(verifier))
        .ok_or(ConnectError::CodeEntryInvalid)?;
    Ok(WaitingCode {
        code: AuthorizationCode(SecretString::from(code.to_owned())),
        verifier: PkceVerifier(SecretString::from(verifier.to_owned())),
    })
}
