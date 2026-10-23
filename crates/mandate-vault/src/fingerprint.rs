//! The account fingerprint (connections spec §3.1, DEC-2345): the keyed hash of a connection's
//! broker account id under the deployment's key, which never leaves the vault. OpenBao's transit
//! engine holds the key, [`FINGERPRINT_KEY`], and computes the hash for the connection's executor
//! at connect; the executor writes it once beside its account entry, and the connection manager
//! reads only the hash and the reference it was computed beside, and claims it in the registry so
//! no two connections hold one account. A fingerprint is pseudonymous personal data: it follows the
//! account entry's retention and erasure (DEC-2339), and prints nothing.

use std::fmt;

use mandate_connections::record::{AccountPiiRef, AccountRef, ConnectionId};
use secrecy::{ExposeSecret, SecretSlice};
use serde_json::Value;

use crate::SourceError;

/// The version of the fingerprint entry's form.
const ENTRY_FORMAT: u8 = 1;

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

/// The connection manager's three fingerprint operations (DEC-2345 items 3 and 4): a trait, so
/// the connect's callers script them in their own tests and drive the vault's through the same
/// calls. [`crate::openbao::OpenBaoSource`] implements it.
#[allow(
    async_fn_in_trait,
    reason = "every caller is in this workspace and awaits on its own runtime; no Send bound is needed"
)]
pub trait ManagerFingerprints {
    /// The fingerprint and reference of the request `account` of `connection`, from the
    /// fingerprint entry its executor wrote at connect; the caller must find the reference equal
    /// to the journaled `account_pii_ref` before it uses the hash (DEC-2345 item 3).
    ///
    /// # Errors
    /// [`FingerprintError::Rotating`] while the key rotates, [`FingerprintError::Missing`] for no
    /// entry, or one that does not read, and [`FingerprintError::Unavailable`] for an outage or a
    /// refused session.
    async fn fingerprint(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<(AccountFingerprint, AccountPiiRef), FingerprintError>;

    /// Claims `fingerprint` for `connection`: [`Claim::Claimed`] when it is now this connection's
    /// or was already, and [`Claim::HeldBy`] another connection that holds it (DEC-2345 item 4).
    ///
    /// # Errors
    /// [`FingerprintError::Rotating`] while the key rotates, and
    /// [`FingerprintError::Unavailable`] for an outage, a refused session, or a registry that
    /// keeps moving under it.
    async fn claim(
        &self,
        connection: &ConnectionId,
        fingerprint: &AccountFingerprint,
    ) -> Result<Claim, FingerprintError>;

    /// Gives back the claim of the request `account` of `connection`, when it holds it: on a
    /// refusal and on a revoke. No fingerprint entry, or no claim held, changes nothing.
    ///
    /// # Errors
    /// [`FingerprintError::Unavailable`] for an outage, a refused session, a fingerprint entry
    /// that does not read, or a registry that keeps moving under it.
    async fn release(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<(), FingerprintError>;
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
/// [`SourceError::Malformed`] if either value cannot be written as JSON text, which neither form
/// allows.
pub fn encode_entry(
    fingerprint: &AccountFingerprint,
    pii_ref: &AccountPiiRef,
) -> Result<SecretSlice<u8>, SourceError> {
    let text = |value: &str| serde_json::to_string(value).map_err(|_| SourceError::Malformed);
    let entry = format!(
        "{{\"fingerprint\":{},\"format\":{ENTRY_FORMAT},\"pii_ref\":{}}}",
        text(fingerprint.as_str())?,
        text(pii_ref.as_str())?
    );
    Ok(SecretSlice::from(entry.into_bytes()))
}

/// The fingerprint and reference a fingerprint entry holds, by its one reader.
///
/// # Errors
/// [`SourceError::Malformed`] for bytes that are not [`encode_entry`]'s form, exactly; it names
/// nothing of them.
pub fn decode_entry(bytes: &[u8]) -> Result<(AccountFingerprint, AccountPiiRef), SourceError> {
    let malformed = SourceError::Malformed;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| malformed.clone())?;
    let member = |name: &str| value.get(name).and_then(Value::as_str);
    let fingerprint = AccountFingerprint::new(member("fingerprint").ok_or(malformed.clone())?)?;
    let pii_ref = AccountPiiRef::new(member("pii_ref").ok_or(malformed.clone())?)
        .map_err(|_| malformed.clone())?;
    let again = encode_entry(&fingerprint, &pii_ref)?;
    if again.expose_secret() == bytes {
        Ok((fingerprint, pii_ref))
    } else {
        Err(malformed)
    }
}
