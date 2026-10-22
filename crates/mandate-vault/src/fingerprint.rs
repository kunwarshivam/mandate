//! The account fingerprint (connections spec §3.1, DEC-2345): the keyed hash of a connection's
//! broker account id under the deployment's key, which never leaves the vault. OpenBao's transit
//! engine holds the key, [`FINGERPRINT_KEY`], and computes the hash for the connection's executor
//! at connect; the executor writes it once beside its account entry, and the connection manager
//! reads only the hash and the reference it was computed beside, and claims it in the registry so
//! no two connections hold one account. A fingerprint is pseudonymous personal data: it follows the
//! account entry's retention and erasure (DEC-2339), and prints nothing.

use std::fmt;

use mandate_connections::record::{AccountPiiRef, ConnectionId};
use secrecy::SecretSlice;

use crate::SourceError;

/// The transit key the fingerprint is computed under: HMAC-SHA256, never exportable.
pub const FINGERPRINT_KEY: &str = "account-fingerprint";

/// The length of a 32-byte HMAC-SHA256 in padded base64: 43 characters and one `=`.
const HASH_LEN: usize = 44;

/// The keyed hash of one account id: transit's HMAC output, `vault:v<key version>:<base64>`.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountFingerprint(String);

impl AccountFingerprint {
    /// The fingerprint `text` is, when it is in transit's HMAC form.
    ///
    /// # Errors
    /// [`SourceError::Malformed`] for any other text; it names nothing of it.
    pub fn new(text: &str) -> Result<Self, SourceError> {
        let in_form = text
            .strip_prefix("vault:v")
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(version, hash)| {
                !version.is_empty()
                    && version.bytes().all(|b| b.is_ascii_digit())
                    && hash.len() == HASH_LEN
                    && hash.strip_suffix('=').is_some_and(|data| {
                        data.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
                    })
            });
        if in_form {
            Ok(Self(text.to_owned()))
        } else {
            Err(SourceError::Malformed)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountFingerprint(..)")
    }
}

/// What a claim of a fingerprint found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The connection holds the fingerprint: claimed now, or held already.
    Claimed,
    /// Another connection holds it: the account is already connected (connections spec check 4).
    HeldBy(ConnectionId),
}

/// Why the vault cannot answer for a fingerprint. Every one of them fails the connect closed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FingerprintError {
    /// The key is rotating: every new connect is refused until each fingerprint is recomputed
    /// (connections spec §3.1).
    Rotating,
    /// The connection has no fingerprint entry.
    Missing,
    /// The vault is sealed or unreachable, or refused the session.
    Unavailable,
}

/// The fingerprint entry's bytes: the fingerprint and the reference of the account entry it was
/// computed beside, in one canonical form, `{"fingerprint":…,"format":1,"pii_ref":…}`.
///
/// # Errors
/// Never for the two values in their forms; the result keeps the stub's place until E21-32 lands.
pub fn encode_entry(
    fingerprint: &AccountFingerprint,
    pii_ref: &AccountPiiRef,
) -> Result<SecretSlice<u8>, SourceError> {
    let _ = (fingerprint, pii_ref);
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// The fingerprint and reference a fingerprint entry holds, by its one reader.
///
/// # Errors
/// [`SourceError::Malformed`] for bytes that are not [`encode_entry`]'s form, exactly; it names
/// nothing of them.
pub fn decode_entry(bytes: &[u8]) -> Result<(AccountFingerprint, AccountPiiRef), SourceError> {
    let _ = bytes;
    Err(SourceError::Unimplemented { story: "E21-32" })
}
