//! The checks either handle makes before it serves anything (DEC-2120 item 1). Every one refuses
//! rather than repairs, as DEC-692 item 3's do.

use std::path::Path;

use crate::AddressError;
use crate::store::{AddressReader, AddressWriter};

/// The systemd credential holding the current address key. Both the API's and the dispatcher's
/// units load it with `LoadCredential=`.
pub const ADDRESS_KEY_CREDENTIAL: &str = "vault-address-key";
/// The retiring address key, present only during a rotation (DEC-2120 item 6).
pub const PREVIOUS_ADDRESS_KEY_CREDENTIAL: &str = "vault-address-key-previous";
/// DEC-692's token key: neither handle opens while it is among the process's credentials.
pub const TOKEN_KEY_CREDENTIAL: &str = "vault-token-key";
/// DEC-692's pending key: the API holds it, so only the reader refuses it.
pub const PENDING_KEY_CREDENTIAL: &str = "vault-pending-key";
/// Environment variables that must never carry a key. Their presence alone refuses to open.
pub const FORBIDDEN_KEY_VARIABLES: [&str; 4] = [
    "MANDATE_VAULT_ADDRESS_KEY",
    "MANDATE_VAULT_KEY",
    "MANDATE_VAULT_PENDING_KEY",
    "MANDATE_VAULT_TOKEN_KEY",
];
/// The address key's length, in bytes.
pub const KEY_LEN: usize = 32;
/// The directory under the vault root that holds one directory per workspace.
pub const ADDRESSES_DIR: &str = "addresses";

/// The numeric ids the layout must carry. On the demo host they are root's uid, `owlhead_api`'s
/// uid, and `owlhead_notify`'s gid; tests pass their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ids {
    pub root_uid: u32,
    pub api_uid: u32,
    pub notify_gid: u32,
}

/// Everything opening reads. The environment is passed in, so the check reads no global state.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub vault_dir: &'a Path,
    pub credentials_dir: &'a Path,
    pub environment: &'a [(String, String)],
    pub ids: Ids,
}

/// Opens the API's handle after DEC-2120 item 1's checks:
///
/// 1. No [`FORBIDDEN_KEY_VARIABLES`] entry is set, whatever its value.
/// 2. [`ADDRESS_KEY_CREDENTIAL`], and [`PREVIOUS_ADDRESS_KEY_CREDENTIAL`] when present, is a
///    regular file, not a symlink, of exactly [`KEY_LEN`] bytes, with no group or other permission
///    bit; [`TOKEN_KEY_CREDENTIAL`] is absent.
/// 3. The vault root is owned by root, `st_mode` exactly `0o040_755` (DEC-692 item 1), and
///    [`ADDRESSES_DIR`], opened relative to it without following a symlink, is owned
///    by the API user and the dispatcher's group, `st_mode` exactly `0o042_710`, and its
///    descriptor is held for every later open (DEC-2120 item 2).
pub fn open_writer(_inputs: &Inputs<'_>) -> Result<AddressWriter, AddressError> {
    Err(AddressError::Unimplemented { story: "E8-14" })
}

/// Opens the dispatcher's handle after the same checks as [`open_writer`], and refuses
/// [`PENDING_KEY_CREDENTIAL`] among its credentials too.
pub fn open_reader(_inputs: &Inputs<'_>) -> Result<AddressReader, AddressError> {
    Err(AddressError::Unimplemented { story: "E8-14" })
}
