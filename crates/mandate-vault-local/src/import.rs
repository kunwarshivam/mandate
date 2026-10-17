//! `mandate vault import alpaca-oauth-client-secret` (DEC-692 item 6): the client secret from
//! stdin only, never argv, stored as the executor and read back through the same checks every
//! read makes, so an import run as the wrong user leaves nothing behind.

use std::io::Read;

use crate::VaultError;
use crate::entry::STORY;
use crate::startup::Ids;
use crate::store::ExecutorVault;

/// The API service user (DEC-692 item 1).
pub const API_USER: &str = "owlhead_api";
/// The executor service user (DEC-692 item 1).
pub const EXEC_USER: &str = "owlhead_exec";

/// Reads the client secret from `source` and stores it under
/// [`crate::entry::ALPACA_CLIENT_SECRET_KEY_REF`]. One trailing `\n` or `\r\n` is dropped; what
/// remains must be 1 to [`crate::store::MAX_SECRET_LEN`] bytes of printable ASCII with no space.
pub fn import_client_secret(
    vault: &ExecutorVault,
    source: &mut impl Read,
) -> Result<(), VaultError> {
    let _ = (vault, source);
    Err(VaultError::Unimplemented { story: STORY })
}

/// The layout's ids from the host's `/etc/passwd` and `/etc/group` text: root is uid 0, and
/// each service user's uid comes from `passwd`, each group's gid from `group`.
pub fn resolve_ids(passwd: &str, group: &str) -> Result<Ids, VaultError> {
    let _ = (passwd, group);
    Err(VaultError::Unimplemented { story: STORY })
}
