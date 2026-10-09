//! The checks the vault makes before it serves anything (DEC-692). Every one refuses rather than
//! repairs: a wrong owner, mode, or key arrangement means the host was set up wrongly, and the
//! vault does not start until it is fixed.

use std::fmt;
use std::path::Path;

use secrecy::SecretBox;

use crate::VaultError;

/// The systemd credential holding the key for `pending/` (authorization codes).
pub const PENDING_KEY_CREDENTIAL: &str = "vault-pending-key";
/// The systemd credential holding the key for `tokens/` (tokens and the client secret). The
/// executor's only; the API process must never be given it.
pub const TOKEN_KEY_CREDENTIAL: &str = "vault-token-key";
/// Environment variables that must never carry a key. Their presence alone refuses startup.
pub const FORBIDDEN_KEY_VARIABLES: [&str; 3] = [
    "MANDATE_VAULT_KEY",
    "MANDATE_VAULT_PENDING_KEY",
    "MANDATE_VAULT_TOKEN_KEY",
];
/// The length of each key, in bytes (ChaCha20-Poly1305).
pub const KEY_LEN: usize = 32;

/// Which process is starting the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The API process (`owlhead_api`): pending key only.
    Api,
    /// The connection's executor (`owlhead_exec`): both keys.
    Executor,
}

/// The numeric ids the layout must carry. On the demo host they are root, `owlhead_api`, and
/// `owlhead_exec`; tests pass their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ids {
    pub root_uid: u32,
    pub api_uid: u32,
    pub api_gid: u32,
    pub exec_uid: u32,
    pub exec_gid: u32,
}

/// Everything startup reads. The environment is passed in, so the check reads no global state.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub role: Role,
    pub vault_dir: &'a Path,
    pub credentials_dir: &'a Path,
    pub environment: &'a [(String, String)],
    pub ids: Ids,
}

/// The keys a started vault holds. The token key is present for the executor only.
pub struct Keys {
    pub pending: SecretBox<[u8; KEY_LEN]>,
    pub token: Option<SecretBox<[u8; KEY_LEN]>>,
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Keys(redacted)")
    }
}

/// Checks the arrangement and loads the keys:
///
/// 1. No [`FORBIDDEN_KEY_VARIABLES`] entry is set, whatever its value.
/// 2. [`PENDING_KEY_CREDENTIAL`] is a regular file of exactly [`KEY_LEN`] bytes in the
///    credentials directory. For [`Role::Executor`] so is [`TOKEN_KEY_CREDENTIAL`]; for
///    [`Role::Api`] that credential must be absent.
/// 3. The vault directory is a real directory (not a symlink) owned by root, mode `0755`;
///    `pending/` is owned by the API user and the executor's group, mode `2770`; `tokens/` is
///    owned by the executor user and the API's group, mode `0730`. Modes compare exactly, the
///    setgid bit included.
pub fn check(inputs: &Inputs<'_>) -> Result<Keys, VaultError> {
    let _ = inputs;
    Err(VaultError::Unimplemented { story: "E10-13" })
}

/// The API process must not be given the token key in any form, a symlink included. Only a
/// missing entry starts the API; any other failure to stat it refuses with [`VaultError::Io`]
/// (AGENTS.md rule 3).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "check calls it once E10-13 lands; until then only its tests do"
    )
)]
pub(crate) fn refuse_token_key(path: &Path) -> Result<(), VaultError> {
    let _ = path;
    Err(VaultError::Unimplemented { story: "E10-13" })
}

/// Reads one key straight into its secret box. The credential must be a regular file (not a
/// symlink) of exactly [`KEY_LEN`] bytes: a missing one is [`VaultError::KeyMissing`], a wrong
/// type or length [`VaultError::KeyMalformed`], and any other failure to stat, open, or read it
/// [`VaultError::Io`].
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "check calls it once E10-13 lands; until then only its tests do"
    )
)]
pub(crate) fn load_key(path: &Path) -> Result<SecretBox<[u8; KEY_LEN]>, VaultError> {
    let _ = path;
    Err(VaultError::Unimplemented { story: "E10-13" })
}
