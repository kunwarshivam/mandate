//! AppRole credentials and the vault's CA read from a mounted Kubernetes Secret (DEC-2630): the
//! kubelet writes each key as a link, `<name> -> ..data/<name>`, with `..data` a link to the
//! current timestamped directory, so a read follows links, but only to a file inside the mount.
//! The file it reaches must be a plain file with no group or other permission bit.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path};

use rustix::fs::{Mode, OFlags, open};
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretString};

use crate::SourceError;
use crate::credentials::{ROLE_ID_SUFFIX, SECRET_ID_SUFFIX};
use crate::policy::{Identity, role_name};

/// The permission bits of group and others, none of which a mounted file may carry.
const OPEN_TO_OTHERS: u32 = 0o077;
/// The most bytes a mounted file may hold: a role id, a secret id or a CA certificate is far
/// shorter. The buffer is sized from the opened file's length, up to this, so the read never
/// reallocates and leaves an unwiped copy behind.
pub const MOUNTED_MAX: u64 = 65536;
/// How a mounted file is opened: read-only (`O_RDONLY`, no bit of its own), never through a link at
/// its last step, never waiting on a pipe, and not inherited by a child.
const OPEN_FLAGS: OFlags = OFlags::NOFOLLOW
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

/// Reads the file `name` of the mounted directory `dir` whole: `name` is one plain path
/// component, and the file it names, its links followed, is a plain file inside `dir` that no
/// group or other may read, write, or run. Anything else, a link that leaves `dir` by a relative
/// or an absolute target included, is [`SourceError::CredentialFile`]. The file is opened without
/// following a link and without blocking, and its type, mode, device and inode are judged on the
/// opened handle against the path checked, so a file, link, or pipe swapped in after the check is
/// refused too. A file of more than [`MOUNTED_MAX`] bytes is refused. A partial read is zeroized
/// before it is dropped.
pub fn read_mounted(dir: &Path, name: &str) -> Result<SecretString, SourceError> {
    read_checked(dir, name, &|_| {})
}

/// [`read_mounted`], calling `between` with the path reached after it is checked and before it is
/// opened, the moment a swap would race.
pub(crate) fn read_checked(
    dir: &Path,
    name: &str,
    between: &dyn Fn(&Path),
) -> Result<SecretString, SourceError> {
    let refused = |_| SourceError::CredentialFile;
    let mut components = Path::new(name).components();
    let one_component = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(only)), None) if only == name
    );
    if !one_component {
        return Err(SourceError::CredentialFile);
    }
    let root = fs::canonicalize(dir).map_err(refused)?;
    let reached = fs::canonicalize(root.join(name)).map_err(refused)?;
    if !reached.starts_with(&root) {
        return Err(SourceError::CredentialFile);
    }
    let checked = fs::symlink_metadata(&reached).map_err(refused)?;
    between(&reached);
    let file = File::from(
        open(&reached, OPEN_FLAGS, Mode::empty()).map_err(|_| SourceError::CredentialFile)?,
    );
    let opened = file.metadata().map_err(refused)?;
    if !opened.is_file()
        || opened.permissions().mode() & OPEN_TO_OTHERS != 0
        || (opened.dev(), opened.ino()) != (checked.dev(), checked.ino())
    {
        return Err(SourceError::CredentialFile);
    }
    let expected = opened.len().min(MOUNTED_MAX).saturating_add(1);
    let mut bytes = Vec::with_capacity(usize::try_from(expected).unwrap_or(0));
    let read = file
        .take(MOUNTED_MAX.saturating_add(1))
        .read_to_end(&mut bytes);
    if read.is_err() || u64::try_from(bytes.len()).map_or(true, |read| read > MOUNTED_MAX) {
        bytes.zeroize();
        return Err(SourceError::CredentialFile);
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(SecretString::from(text)),
        Err(invalid) => {
            invalid.into_bytes().zeroize();
            Err(SourceError::CredentialFile)
        }
    }
}

/// `value` as one line: at most one trailing newline, which is dropped, and no other line break
/// or carriage return, and not empty.
fn one_line(value: &SecretString) -> Result<SecretString, SourceError> {
    let text = value.expose_secret();
    let line = text.strip_suffix('\n').unwrap_or(text);
    if line.is_empty() || line.contains(['\n', '\r']) {
        return Err(SourceError::CredentialFile);
    }
    Ok(SecretString::from(line.to_owned()))
}

/// Reads `identity`'s AppRole credentials from the mounted directory `dir`, by
/// [`read_mounted`]: `<role>.role-id` and `<role>.secret-id`, each one line, with at most one
/// trailing newline, which is dropped. An empty value, a second line, or a carriage return is
/// [`SourceError::CredentialFile`]. A token exchange is [`SourceError::Config`]: its credential is
/// a single-use grant, never mounted.
pub fn read_mounted_credentials(
    dir: &Path,
    identity: &Identity,
) -> Result<(String, SecretString), SourceError> {
    if let Identity::TokenExchange(_) = identity {
        return Err(SourceError::Config);
    }
    let role = role_name(identity);
    let role_id = one_line(&read_mounted(dir, &format!("{role}{ROLE_ID_SUFFIX}"))?)?;
    let secret_id = one_line(&read_mounted(dir, &format!("{role}{SECRET_ID_SUFFIX}"))?)?;
    Ok((role_id.expose_secret().to_owned(), secret_id))
}
