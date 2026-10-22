//! AppRole credentials and the vault's CA read from a mounted Kubernetes Secret (DEC-2630): the
//! kubelet writes each key as a link, `<name> -> ..data/<name>`, with `..data` a link to the
//! current timestamped directory, so a read follows links, but only to a file inside the mount.
//! The file it reaches must be a plain file with no group or other permission bit, but for the
//! public files of [`read_mounted_ca`] and the projected token, which refuse only a group or other
//! write bit.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path};

use rustix::fs::{Mode, OFlags, open};
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretString};

use crate::SourceError;
use crate::credentials::{ROLE_ID_SUFFIX, SECRET_ID_SUFFIX};
use crate::kube::is_label;
use crate::policy::{Identity, role_name};

/// The permission bits of group and others, none of which a mounted file may carry.
const OPEN_TO_OTHERS: u32 = 0o077;
/// The permission bits that let a group or others write a file.
const WRITABLE_BY_OTHERS: u32 = 0o022;
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
    read_bounded(dir, name, between, OPEN_TO_OTHERS, MOUNTED_MAX)
}

/// [`read_checked`] with the mode bits that refuse the file and its most bytes given.
fn read_bounded(
    dir: &Path,
    name: &str,
    between: &dyn Fn(&Path),
    refused_bits: u32,
    max: u64,
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
        || opened.permissions().mode() & refused_bits != 0
        || (opened.dev(), opened.ino()) != (checked.dev(), checked.ino())
    {
        return Err(SourceError::CredentialFile);
    }
    let expected = opened.len().min(max).saturating_add(1);
    let mut bytes = Vec::with_capacity(usize::try_from(expected).unwrap_or(0));
    let read = file.take(max.saturating_add(1)).read_to_end(&mut bytes);
    if read.is_err() || u64::try_from(bytes.len()).map_or(true, |read| read > max) {
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

/// The one file [`read_mounted_ca`] reads: the vault's CA certificate, which is public.
pub const CA_FILE: &str = "ca.crt";

/// Reads the vault's CA certificate, `dir/ca.crt`, whole, by [`read_mounted`]'s rules but its
/// mode (DEC-2340): a CA is public, and what matters is that no one but its owner can change it,
/// so only a group or other write bit is refused, as for [`read_service_account_token`]. A
/// ConfigMap mounted at 0444, or at 0440 under an `fsGroup`, is read. No other file is read this
/// way: a role id, a secret id or a key keeps [`read_mounted`]'s owner-only rule. Anything else is
/// [`SourceError::CredentialFile`].
pub fn read_mounted_ca(dir: &Path) -> Result<String, SourceError> {
    read_bounded(dir, CA_FILE, &|_| {}, WRITABLE_BY_OTHERS, MOUNTED_MAX)
        .map(|ca| ca.expose_secret().to_owned())
}

/// Where a connection's executor or token exchange finds its projected ServiceAccount token, the
/// file `token` in it (DEC-2060): projected for the audience [`crate::kubernetes::AUDIENCE`],
/// lasting 600 seconds, and rotated by the kubelet, so it is read again at each login.
pub const SERVICE_ACCOUNT_TOKEN_DIR: &str = "/run/mandate/vault-token";

/// Reads the projected ServiceAccount token `dir/token` by [`read_mounted`]'s rules but its mode:
/// the kubelet writes a projected token readable by all, and it is short-lived and bound to the
/// vault's audience, so only a group or other write bit is refused. It must be one line, at most
/// one trailing newline, no carriage return, and no longer than [`SERVICE_ACCOUNT_TOKEN_MAX`]
/// bytes. Anything else is [`SourceError::CredentialFile`].
pub fn read_service_account_token(dir: &Path) -> Result<SecretString, SourceError> {
    let token = read_bounded(
        dir,
        "token",
        &|_| {},
        WRITABLE_BY_OTHERS,
        SERVICE_ACCOUNT_TOKEN_MAX,
    )?;
    one_line(&token)
}

/// Reads the pod's own namespace, `dir/namespace`, which the projected volume holds beside the
/// token from the downward API (`metadata.namespace`), so no pod names its namespace in its
/// arguments: by [`read_service_account_token`]'s rules, at most 64 bytes, and a Kubernetes label.
/// Anything else is [`SourceError::CredentialFile`].
pub fn read_service_account_namespace(dir: &Path) -> Result<String, SourceError> {
    let namespace = read_bounded(
        dir,
        "namespace",
        &|_| {},
        WRITABLE_BY_OTHERS,
        SERVICE_ACCOUNT_NAMESPACE_MAX,
    )?;
    let line = one_line(&namespace)?;
    let text = line.expose_secret();
    if is_label(text) {
        Ok(text.to_owned())
    } else {
        Err(SourceError::CredentialFile)
    }
}

/// The most bytes the projected namespace may hold: a 63-byte label and its newline.
const SERVICE_ACCOUNT_NAMESPACE_MAX: u64 = 64;

/// The most bytes a projected ServiceAccount token may hold; a real one is about a kilobyte.
pub const SERVICE_ACCOUNT_TOKEN_MAX: u64 = 16384;

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
