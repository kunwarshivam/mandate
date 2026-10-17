//! The encrypted reads and writes of `pending/` and `tokens/` (DEC-692 items 4 and 5).
//!
//! Each entry is one file: a four-byte format tag, a random 12-byte nonce, and the
//! ChaCha20-Poly1305 ciphertext with its tag, under the pending key for a code and the token key
//! for a token or the client secret, with the entry's [`Entry::associated_data`].
//!
//! A write goes to a fresh temporary file in the same directory, opened with `O_EXCL`, set to
//! its exact mode on the open file, synced, and renamed over the entry, so a reader sees the old
//! entry or the new one and never a part.
//!
//! A read checks the file before it decrypts anything. The name must be a regular file, not a
//! symlink; the open file must be that same inode (same device and inode number as the name's
//! `lstat`), so a name swapped between the check and the open is refused; and the open file must
//! have exactly one link, the owner and mode its kind requires, and a size the format allows:
//!
//! | Kind | Owner | Group | `st_mode` |
//! |---|---|---|---|
//! | Code | the API user | the executor's group | `0100640` |
//! | Token, client secret | the executor user | any | `0100600` |

use std::fmt;
use std::fs::Metadata;
use std::path::PathBuf;

use secrecy::{SecretBox, SecretSlice};

use crate::VaultError;
use crate::entry::{Entry, Kind, STORY};
use crate::startup::{Ids, Inputs, KEY_LEN};

/// The longest secret an entry holds, in bytes.
pub const MAX_SECRET_LEN: usize = 8192;
/// The format tag every entry file starts with.
pub const FORMAT_TAG: [u8; 4] = *b"MVL\x01";
/// The nonce length of ChaCha20-Poly1305, in bytes.
pub const NONCE_LEN: usize = 12;
/// The authentication tag length of ChaCha20-Poly1305, in bytes.
pub const TAG_LEN: usize = 16;

/// The vault as the API process holds it: the pending key only. It writes codes and unlinks a
/// code or a token by name; it reads nothing.
pub struct ApiVault {
    vault_dir: PathBuf,
    ids: Ids,
    pending_key: SecretBox<[u8; KEY_LEN]>,
}

/// The vault as the executor holds it: both keys. It reads codes, and writes and reads tokens
/// and the client secret.
pub struct ExecutorVault {
    vault_dir: PathBuf,
    ids: Ids,
    pending_key: SecretBox<[u8; KEY_LEN]>,
    token_key: SecretBox<[u8; KEY_LEN]>,
}

/// A started vault, in the role its inputs named.
#[derive(Debug)]
pub enum Opened {
    Api(ApiVault),
    Executor(ExecutorVault),
}

impl fmt::Debug for ApiVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiVault(redacted)")
    }
}

impl fmt::Debug for ExecutorVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExecutorVault(redacted)")
    }
}

/// Runs [`crate::startup::check`] and returns the vault in the role `inputs` names.
pub fn open(inputs: &Inputs<'_>) -> Result<Opened, VaultError> {
    let _ = inputs;
    Err(VaultError::Unimplemented { story: STORY })
}

impl Opened {
    /// The executor's vault, or [`VaultError::WrongRole`] for the API's.
    pub fn into_executor(self) -> Result<ExecutorVault, VaultError> {
        Err(VaultError::Unimplemented { story: STORY })
    }

    /// The API's vault, or [`VaultError::WrongRole`] for the executor's.
    pub fn into_api(self) -> Result<ApiVault, VaultError> {
        Err(VaultError::Unimplemented { story: STORY })
    }
}

impl ApiVault {
    /// Writes a code entry, replacing any earlier one. Any other kind is
    /// [`VaultError::WrongRole`].
    pub fn put_code(&self, entry: &Entry, secret: &SecretSlice<u8>) -> Result<(), VaultError> {
        let _ = (entry, secret, &self.vault_dir, &self.ids, &self.pending_key);
        Err(VaultError::Unimplemented { story: STORY })
    }

    /// Unlinks a code or a token by name; an absent entry is already deleted. The client secret
    /// is [`VaultError::WrongRole`].
    pub fn delete(&self, entry: &Entry) -> Result<(), VaultError> {
        let _ = entry;
        Err(VaultError::Unimplemented { story: STORY })
    }
}

impl ExecutorVault {
    /// Writes a token or the client secret, replacing any earlier one. A code is
    /// [`VaultError::WrongRole`]: only the API writes codes, so only its files pass a code read.
    pub fn put(&self, entry: &Entry, secret: &SecretSlice<u8>) -> Result<(), VaultError> {
        let _ = (
            entry,
            secret,
            &self.vault_dir,
            &self.ids,
            &self.pending_key,
            &self.token_key,
        );
        Err(VaultError::Unimplemented { story: STORY })
    }

    /// Reads and decrypts an entry of any kind; `None` when it is absent.
    pub fn read(&self, entry: &Entry) -> Result<Option<SecretSlice<u8>>, VaultError> {
        let _ = entry;
        Err(VaultError::Unimplemented { story: STORY })
    }

    /// Unlinks an entry of any kind; an absent entry is already deleted.
    pub fn delete(&self, entry: &Entry) -> Result<(), VaultError> {
        let _ = entry;
        Err(VaultError::Unimplemented { story: STORY })
    }
}

/// Checks an entry's file before any byte of it is decrypted: `named` is the name's `lstat`,
/// `opened` the open file's `fstat`. They must be one inode, a regular file with one link, at
/// the owner and `st_mode` the table above gives `kind`.
pub(crate) fn check_open_file(
    kind: Kind,
    ids: &Ids,
    named: &Metadata,
    opened: &Metadata,
) -> Result<(), VaultError> {
    let _ = (kind, ids, named, opened);
    Err(VaultError::Unimplemented { story: STORY })
}
