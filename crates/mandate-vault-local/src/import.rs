//! `mandate vault import alpaca-oauth-client-secret` (DEC-692 item 6): the client secret from
//! stdin only, never argv, stored as the executor and read back through the same checks every
//! read makes, so an import run as the wrong user leaves nothing behind.

use std::io::Read;

use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretSlice};

use crate::VaultError;
use crate::entry::{ALPACA_CLIENT_SECRET_KEY_REF, Entry, KeyRef};
use crate::startup::Ids;
use crate::store::{ExecutorVault, MAX_SECRET_LEN};

/// The API service user (DEC-692 item 1).
pub const API_USER: &str = "owlhead_api";
/// The executor service user (DEC-692 item 1).
pub const EXEC_USER: &str = "owlhead_exec";

/// Reads the client secret from `source` and stores it under
/// [`crate::entry::ALPACA_CLIENT_SECRET_KEY_REF`]. One trailing `\n` or `\r\n` is dropped; what
/// remains must be 1 to [`crate::store::MAX_SECRET_LEN`] bytes of printable ASCII with no space.
///
/// The stored entry is read back through [`ExecutorVault::read`], so an import that ran as
/// another user than the executor, whose file the read's owner check refuses, is deleted and
/// refused.
pub fn import_client_secret(
    vault: &ExecutorVault,
    source: &mut impl Read,
) -> Result<(), VaultError> {
    let secret = read_line(source)?;
    let entry = Entry::client_secret(KeyRef::new(ALPACA_CLIENT_SECRET_KEY_REF)?);
    vault.put(&entry, &secret)?;
    let read_back = vault.read(&entry);
    let matches =
        matches!(&read_back, Ok(Some(stored)) if stored.expose_secret() == secret.expose_secret());
    if matches {
        Ok(())
    } else {
        let _ = vault.delete(&entry);
        Err(read_back.err().unwrap_or(VaultError::Io))
    }
}

/// The longest input read: the longest secret and a `\r\n`. One byte more is read, so a longer
/// input is seen as longer without reading all of it.
const MAX_INPUT_LEN: usize = MAX_SECRET_LEN + 2;

/// Reads one line of printable bytes, its one line ending dropped, from at most one byte more
/// than [`MAX_INPUT_LEN`]. The buffer holds that much from the start, so reading never moves the
/// secret, and it is zeroed whatever the outcome. Its length is left to [`ExecutorVault::put`],
/// which refuses an empty or too-long secret before it writes anything.
fn read_line(source: &mut impl Read) -> Result<SecretSlice<u8>, VaultError> {
    let limit = MAX_INPUT_LEN.saturating_add(1);
    let mut buffer = Vec::with_capacity(limit);
    let read = source
        .take(u64::try_from(limit).unwrap_or(u64::MAX))
        .read_to_end(&mut buffer);
    let line = read.map_err(|_| VaultError::Io).and_then(|_| {
        let line = buffer
            .strip_suffix(b"\r\n")
            .or_else(|| buffer.strip_suffix(b"\n"))
            .unwrap_or(&buffer);
        if buffer.len() > MAX_INPUT_LEN {
            Err(VaultError::SecretSize)
        } else if line.iter().all(|b| (0x21..=0x7E).contains(b)) {
            Ok(SecretSlice::from(line.to_vec()))
        } else {
            Err(VaultError::SecretNotPrintable)
        }
    });
    buffer.zeroize();
    line
}

/// The superuser that owns the vault directory (DEC-692 item 1).
pub const ROOT_USER: &str = "root";

/// The layout's ids from the host's `/etc/passwd` and `/etc/group` text: each user's uid comes
/// from `passwd` (`root` included, so a test host can name its own user), each group's gid from
/// `group`.
/// The first line naming a user or group is taken, as `getpwnam` does.
pub fn resolve_ids(passwd: &str, group: &str) -> Result<Ids, VaultError> {
    Ok(Ids {
        root_uid: id_of(passwd, ROOT_USER)?,
        api_uid: id_of(passwd, API_USER)?,
        api_gid: id_of(group, API_USER)?,
        exec_uid: id_of(passwd, EXEC_USER)?,
        exec_gid: id_of(group, EXEC_USER)?,
    })
}

/// The third field of the first `name:` line, as a decimal id.
fn id_of(database: &str, name: &'static str) -> Result<u32, VaultError> {
    database
        .lines()
        .find_map(|line| {
            let mut fields = line.split(':');
            (fields.next() == Some(name)).then(|| fields.nth(1))
        })
        .flatten()
        .filter(|id| id.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|id| id.parse().ok())
        .ok_or(VaultError::UnknownUser { name })
}
