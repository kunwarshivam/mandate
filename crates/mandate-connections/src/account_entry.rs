//! The personal-data vault's account entry, format 1
//! ([DEC-2339](../../../docs/project/decisions/DEC-2339.md)): the bytes a connection's executor
//! creates once at `personal/accounts/x<hex>`, and the connection manager reads to have the
//! account fingerprint computed (connections spec §3.1, §5.2 step 4). They are exactly the
//! canonical JSON (journal spec §4)
//!
//! `{"account_id":"<id>","format":1,"pii_ref":"<ref>"}`
//!
//! with an id of 1 to [`MAX_ACCOUNT_ID_LEN`] bytes from `!` to `~` other than `"` and `\`, so it
//! needs no escape and the entry has one byte form, and a reference that is `pii_` and a ULID
//! (journal spec §6.4). The id is personal data: it is only ever a [`BrokerAccountId`], which no
//! `Debug`, draft or log prints, and leaves the process only into the vault.

use std::fmt;

use mandate_canon::is_ulid;
use secrecy::{ExposeSecret, SecretString};

use crate::ConnectError;
use crate::record::AccountPiiRef;

/// The entry's format.
pub const FORMAT: u64 = 1;

/// The longest account id an entry holds, in bytes.
pub const MAX_ACCOUNT_ID_LEN: usize = 128;

/// The entry's bytes before the id.
const BEFORE: &str = r#"{"account_id":""#;

/// The entry's bytes between the id and the reference.
const BETWEEN: &str = r#"","format":1,"pii_ref":""#;

/// The entry's bytes after the reference.
const AFTER: &str = r#""}"#;

/// Whether `id` is 1 to [`MAX_ACCOUNT_ID_LEN`] bytes from `!` to `~`, none of them `"` or `\`.
fn in_form(id: &str) -> bool {
    (1..=MAX_ACCOUNT_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|b| (b'!'..=b'~').contains(&b) && b != b'"' && b != b'\\')
}

/// Whether `pii_ref` is `pii_` and a ULID (journal spec §6.4).
fn is_pii_ref(pii_ref: &str) -> bool {
    pii_ref.strip_prefix("pii_").is_some_and(is_ulid)
}

/// A broker's own id of the account a credential reaches: personal data (journal spec §6.4),
/// read once through the connection's paper host, written once to the personal-data vault, and
/// never printed. The type the executor's connect check and the connection manager share.
pub struct BrokerAccountId(pub(crate) SecretString);

impl fmt::Debug for BrokerAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrokerAccountId(redacted)")
    }
}

impl PartialEq for BrokerAccountId {
    fn eq(&self, other: &Self) -> bool {
        self.0.expose_secret() == other.0.expose_secret()
    }
}

impl Eq for BrokerAccountId {}

impl BrokerAccountId {
    /// The id `text` names, when it is in the entry's form.
    ///
    /// # Errors
    /// [`ConnectError::AccountEntryInvalid`] for any other text; the error names nothing of it.
    pub fn new(text: &str) -> Result<Self, ConnectError> {
        if in_form(text) {
            Ok(Self(SecretString::from(text.to_owned())))
        } else {
            Err(ConnectError::AccountEntryInvalid)
        }
    }
}

/// One decoded entry: the reference the executor minted, and the id it stands for.
#[derive(Debug)]
pub struct AccountEntry {
    pub pii_ref: AccountPiiRef,
    pub account_id: BrokerAccountId,
}

/// The entry for `account_id` under `pii_ref`.
///
/// # Errors
/// [`ConnectError::AccountEntryInvalid`] for a reference outside the entry's form.
pub fn encode(
    pii_ref: &AccountPiiRef,
    account_id: &BrokerAccountId,
) -> Result<SecretString, ConnectError> {
    let id = account_id.0.expose_secret();
    let pii_ref = pii_ref.as_str();
    if !in_form(id) || !is_pii_ref(pii_ref) {
        return Err(ConnectError::AccountEntryInvalid);
    }
    Ok(SecretString::from(format!(
        "{BEFORE}{id}{BETWEEN}{pii_ref}{AFTER}"
    )))
}

/// The reference and id of an entry's bytes.
///
/// # Errors
/// [`ConnectError::AccountEntryInvalid`] for any bytes but format 1's exact form; the error
/// names nothing of the bytes.
pub fn decode(bytes: &[u8]) -> Result<AccountEntry, ConnectError> {
    let (id, pii_ref) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_prefix(BEFORE))
        .and_then(|rest| rest.strip_suffix(AFTER))
        .and_then(|rest| rest.split_once(BETWEEN))
        .filter(|(id, pii_ref)| in_form(id) && is_pii_ref(pii_ref))
        .ok_or(ConnectError::AccountEntryInvalid)?;
    Ok(AccountEntry {
        pii_ref: AccountPiiRef::new(pii_ref).map_err(|_| ConnectError::AccountEntryInvalid)?,
        account_id: BrokerAccountId(SecretString::from(id.to_owned())),
    })
}
